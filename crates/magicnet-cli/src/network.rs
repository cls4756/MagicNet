use std::fs;
use std::io;
use std::os::fd::RawFd;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use serde_json::Value;

use crate::service::restart_current_core;
use crate::{read_kv, run_magicnet_function, write_kv, App};

const NETWORK_POLICY_CONF: &str = ".config/magicnet/network-policy.conf";
const DEFAULT_IPV6_MODE: &str = "prefer_ipv4";
const DEFAULT_MTU: u16 = 1400;
const DEFAULT_UDP_TIMEOUT: &str = "5m";
const DNS_CONF: &str = ".config/magicnet/dns.conf";
const NETWORK_EVENT_DEBOUNCE: Duration = Duration::from_millis(750);
const NETWORK_DNS_CONFIRMATION_DELAY: Duration = Duration::from_millis(500);
const NETWORK_FALLBACK_INTERVAL: Duration = Duration::from_secs(60);

const NETLINK_ROUTE: libc::c_int = 0;
const NETLINK_GROUP_LINK: u32 = 1;
const NETLINK_GROUP_IPV4_IFADDR: u32 = 0x10;
const NETLINK_GROUP_IPV4_ROUTE: u32 = 0x40;
const NETLINK_GROUP_IPV6_IFADDR: u32 = 0x100;
const NETLINK_GROUP_IPV6_ROUTE: u32 = 0x400;

#[repr(C)]
struct NetlinkSocketAddress {
    nl_family: libc::sa_family_t,
    nl_pad: u16,
    nl_pid: u32,
    nl_groups: u32,
}

struct NetworkEventSource {
    fd: RawFd,
}

impl NetworkEventSource {
    fn new() -> io::Result<Self> {
        let fd = unsafe {
            libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_RAW | libc::SOCK_CLOEXEC,
                NETLINK_ROUTE,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        let groups = NETLINK_GROUP_LINK
            | NETLINK_GROUP_IPV4_IFADDR
            | NETLINK_GROUP_IPV4_ROUTE
            | NETLINK_GROUP_IPV6_IFADDR
            | NETLINK_GROUP_IPV6_ROUTE;
        let address = NetlinkSocketAddress {
            nl_family: libc::AF_NETLINK as libc::sa_family_t,
            nl_pad: 0,
            nl_pid: 0,
            nl_groups: groups,
        };
        let bind_result = unsafe {
            libc::bind(
                fd,
                (&address as *const NetlinkSocketAddress).cast::<libc::sockaddr>(),
                std::mem::size_of::<NetlinkSocketAddress>() as libc::socklen_t,
            )
        };
        if bind_result < 0 {
            let error = io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(error);
        }

        Ok(Self { fd })
    }

    fn wait(&self, timeout: Duration) -> io::Result<bool> {
        let timeout_ms = timeout
            .as_millis()
            .min(libc::c_int::MAX as u128)
            .try_into()
            .unwrap_or(libc::c_int::MAX);
        let mut pollfd = libc::pollfd {
            fd: self.fd,
            events: libc::POLLIN | libc::POLLERR | libc::POLLHUP,
            revents: 0,
        };

        loop {
            let result = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if result == 0 {
                return Ok(false);
            }
            if pollfd.revents & libc::POLLNVAL != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "netlink event socket became invalid",
                ));
            }
            self.drain();
            return Ok(true);
        }
    }

    fn drain(&self) {
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            let received = unsafe {
                libc::recv(
                    self.fd,
                    buffer.as_mut_ptr().cast::<libc::c_void>(),
                    buffer.len(),
                    libc::MSG_DONTWAIT,
                )
            };
            if received > 0 {
                continue;
            }
            if received < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
            }
            return;
        }
    }
}

impl Drop for NetworkEventSource {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NetworkPolicy {
    ipv6_mode: &'static str,
    mtu: u16,
    udp_timeout: &'static str,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            ipv6_mode: DEFAULT_IPV6_MODE,
            mtu: DEFAULT_MTU,
            udp_timeout: DEFAULT_UDP_TIMEOUT,
        }
    }
}

pub(crate) fn network_cmd(app: &App, args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str).unwrap_or("status") {
        "status" => {
            print_status(app, &NetworkPolicy::load(app));
            Ok(())
        }
        "set" => {
            let policy = NetworkPolicy::from_args(&args[1..])?;
            policy.write(app)?;
            apply_network_policy(app)?;
            print_status(app, &policy);
            println!("[info] Network policy applied");
            Ok(())
        }
        "apply" => {
            apply_network_policy(app)?;
            print_status(app, &NetworkPolicy::load(app));
            Ok(())
        }
        "watch" => network_watch(app),
        _ => Err(network_usage()),
    }
}

fn network_watch(app: &App) -> Result<(), String> {
    let event_source = match NetworkEventSource::new() {
        Ok(source) => Some(source),
        Err(error) => {
            eprintln!("[warn] network event listener unavailable; using fallback checks: {error}");
            None
        }
    };
    let mut applied_signature: Option<String> = None;

    loop {
        if app.moddir.join("disable").exists() || app.moddir.join("remove").exists() {
            return Ok(());
        }

        if !dns_bootstrap_is_system(app) {
            return Ok(());
        }

        if applied_signature.is_none() {
            applied_signature = android_dns_signature();
        }

        let event_received = wait_for_network_change(event_source.as_ref());
        if event_received {
            thread::sleep(NETWORK_EVENT_DEBOUNCE);
        }

        if app.moddir.join("disable").exists() || app.moddir.join("remove").exists() {
            return Ok(());
        }
        if !dns_bootstrap_is_system(app) {
            applied_signature = None;
            continue;
        }

        let Some(signature) = android_dns_signature() else {
            continue;
        };
        if applied_signature.as_deref() == Some(signature.as_str()) {
            continue;
        }

        thread::sleep(NETWORK_DNS_CONFIRMATION_DELAY);
        let Some(confirmed_signature) = android_dns_signature() else {
            continue;
        };
        if confirmed_signature != signature {
            continue;
        }

        match run_magicnet_function(app, "magicnet_dns_apply")
            .and_then(|_| restart_current_core(app))
        {
            Ok(()) => {
                applied_signature = Some(confirmed_signature);
            }
            Err(error) => {
                eprintln!("[warn] Android DNS change could not be applied: {error}");
            }
        }
    }
}

fn wait_for_network_change(event_source: Option<&NetworkEventSource>) -> bool {
    match event_source {
        Some(source) => match source.wait(NETWORK_FALLBACK_INTERVAL) {
            Ok(event_received) => event_received,
            Err(error) => {
                eprintln!("[warn] network event wait failed; using fallback checks: {error}");
                thread::sleep(NETWORK_FALLBACK_INTERVAL);
                false
            }
        },
        None => {
            thread::sleep(NETWORK_FALLBACK_INTERVAL);
            false
        }
    }
}

fn dns_bootstrap_is_system(app: &App) -> bool {
    read_kv(app.moddir.join(DNS_CONF))
        .get("MAGICNET_BOOTSTRAP_DNS")
        .is_some_and(|value| matches!(value.as_str(), "system" | "android" | "android-system"))
}

fn android_dns_signature() -> Option<String> {
    let connectivity = Command::new("dumpsys")
        .arg("connectivity")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    let properties = (1..=4)
        .filter_map(|index| {
            Command::new("getprop")
                .arg(format!("net.dns{index}"))
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        })
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    let mut lines = connectivity
        .lines()
        .filter(|line| line.contains("DnsAddresses:"))
        .map(str::trim)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    lines.extend(properties);
    lines.sort();
    lines.dedup();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn apply_network_policy(app: &App) -> Result<(), String> {
    run_magicnet_function(app, "magicnet_transparent_apply")?;
    // sing-box reads the TUN inbound and DNS strategy at process start; a
    // successful file rewrite alone leaves a running core on the old policy.
    restart_current_core(app)
}

impl NetworkPolicy {
    fn load(app: &App) -> Self {
        let values = read_kv(app.moddir.join(NETWORK_POLICY_CONF));
        Self {
            ipv6_mode: normalize_ipv6_mode(
                values
                    .get("MAGICNET_IPV6_MODE")
                    .map(String::as_str)
                    .unwrap_or_default(),
            )
            .unwrap_or(DEFAULT_IPV6_MODE),
            mtu: normalize_mtu(
                values
                    .get("MAGICNET_TUN_MTU")
                    .map(String::as_str)
                    .unwrap_or_default(),
            )
            .unwrap_or(DEFAULT_MTU),
            udp_timeout: normalize_udp_timeout(
                values
                    .get("MAGICNET_UDP_TIMEOUT")
                    .map(String::as_str)
                    .unwrap_or_default(),
            )
            .unwrap_or(DEFAULT_UDP_TIMEOUT),
        }
    }

    fn from_args(args: &[String]) -> Result<Self, String> {
        if args.len() != 3 {
            return Err(network_usage());
        }
        Ok(Self {
            ipv6_mode: normalize_ipv6_mode(&args[0]).ok_or_else(network_usage)?,
            mtu: normalize_mtu(&args[1]).ok_or_else(network_usage)?,
            udp_timeout: normalize_udp_timeout(&args[2]).ok_or_else(network_usage)?,
        })
    }

    fn write(&self, app: &App) -> Result<(), String> {
        write_kv(
            app,
            Path::new(NETWORK_POLICY_CONF),
            &[
                ("MAGICNET_IPV6_MODE", self.ipv6_mode.to_string()),
                ("MAGICNET_TUN_MTU", self.mtu.to_string()),
                ("MAGICNET_UDP_TIMEOUT", self.udp_timeout.to_string()),
            ],
        )
    }
}

fn normalize_ipv6_mode(value: &str) -> Option<&'static str> {
    match value {
        "ipv4_only" | "ipv4-only" | "compat" | "disabled" => Some("ipv4_only"),
        "prefer_ipv4" | "prefer-ipv4" | "auto" | "dual" => Some("prefer_ipv4"),
        "prefer_ipv6" | "prefer-ipv6" => Some("prefer_ipv6"),
        _ => None,
    }
}

fn normalize_mtu(value: &str) -> Option<u16> {
    value
        .parse::<u16>()
        .ok()
        .filter(|value| (1280..=1500).contains(value))
}

fn normalize_udp_timeout(value: &str) -> Option<&'static str> {
    match value {
        "1m" => Some("1m"),
        "3m" => Some("3m"),
        "5m" => Some("5m"),
        "10m" => Some("10m"),
        "15m" => Some("15m"),
        "30m" => Some("30m"),
        _ => None,
    }
}

fn print_status(app: &App, policy: &NetworkPolicy) {
    println!("ipv6_mode={}", policy.ipv6_mode);
    println!("mtu={}", policy.mtu);
    println!("udp_timeout={}", policy.udp_timeout);

    let effective = fs::read_to_string(app.moddir.join(".config/sing-box/config.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let tun = effective
        .as_ref()
        .and_then(|config| config.get("inbounds"))
        .and_then(Value::as_array)
        .and_then(|inbounds| {
            inbounds
                .iter()
                .find(|inbound| inbound.get("type").and_then(Value::as_str) == Some("tun"))
        });
    let strategy = effective
        .as_ref()
        .and_then(|config| config.get("dns"))
        .and_then(|dns| dns.get("strategy"))
        .and_then(Value::as_str)
        .unwrap_or("unavailable");
    println!("effective_ipv6_mode={strategy}");
    println!(
        "effective_stack={}",
        tun.and_then(|tun| tun.get("stack"))
            .and_then(Value::as_str)
            .unwrap_or("unavailable")
    );
    println!(
        "effective_mtu={}",
        tun.and_then(|tun| tun.get("mtu"))
            .and_then(Value::as_u64)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "effective_udp_timeout={}",
        tun.and_then(|tun| tun.get("udp_timeout"))
            .and_then(Value::as_str)
            .unwrap_or("unavailable")
    );
}

fn network_usage() -> String {
    "Usage: cli network {status|set <ipv4_only|prefer_ipv4|prefer_ipv6> <mtu:1280-1500> <udp-timeout:1m|3m|5m|10m|15m|30m>|apply|watch}".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_normalize_to_canonical_ipv6_modes() {
        assert_eq!(normalize_ipv6_mode("compat"), Some("ipv4_only"));
        assert_eq!(normalize_ipv6_mode("dual"), Some("prefer_ipv4"));
        assert_eq!(normalize_ipv6_mode("prefer-ipv6"), Some("prefer_ipv6"));
        assert_eq!(normalize_ipv6_mode("ipv6_only"), None);
    }

    #[test]
    fn mtu_rejects_values_that_break_ipv6_or_exceed_common_links() {
        assert_eq!(normalize_mtu("1280"), Some(1280));
        assert_eq!(normalize_mtu("1400"), Some(1400));
        assert_eq!(normalize_mtu("1500"), Some(1500));
        assert_eq!(normalize_mtu("1279"), None);
        assert_eq!(normalize_mtu("1501"), None);
    }

    #[test]
    fn udp_timeout_uses_bounded_presets() {
        assert_eq!(normalize_udp_timeout("5m"), Some("5m"));
        assert_eq!(normalize_udp_timeout("30m"), Some("30m"));
        assert_eq!(normalize_udp_timeout("0m"), None);
        assert_eq!(normalize_udp_timeout("1h"), None);
    }
}
