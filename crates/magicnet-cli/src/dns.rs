use std::fs;
use std::path::Path;
use std::process::Command;

use crate::service::restart_current_core;
use crate::{read_kv, run_magicnet_function, write_text_file, App};

const DNS_CONF: &str = ".config/magicnet/dns.conf";
const DNS_TEST_PROXY: &str = "http://127.0.0.1:7892";

pub(crate) fn dns_cmd(app: &App, args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str).unwrap_or("status") {
        "status" => {
            dns_status(app);
            Ok(())
        }
        "set" => dns_set(app, args.get(1).map(String::as_str).unwrap_or_default()),
        "bootstrap" => dns_bootstrap_cmd(app, &args[1..]),
        "test" => dns_test(args.get(1).map(String::as_str).unwrap_or("www.gstatic.com")),
        "apply" => {
            run_magicnet_function(app, "magicnet_dns_apply")?;
            restart_current_core(app)
        }
        _ => Err(dns_usage()),
    }
}

fn dns_test(domain: &str) -> Result<(), String> {
    let domain = normalize_test_domain(domain)?;
    let url = format!("https://{domain}/");
    let output = Command::new("curl")
        // The CLI runs as uid 0, which is intentionally excluded from
        // magicnet0 to prevent proxy loops.  Force this diagnostic through
        // the local mixed inbound so it measures the running sing-box path
        // instead of bypassing the tunnel and producing a false timeout.
        .args(dns_test_curl_args(&url))
        .output()
        .map_err(|err| format!("run curl: {err}"))?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn dns_test_curl_args(url: &str) -> Vec<&str> {
    vec![
        // Root is deliberately outside the magicnet0 TUN boundary. Route the
        // probe through MagicNet's loopback mixed inbound so it exercises the
        // same managed DNS and outbound path as proxied applications.
        // A DNS/transport probe must not fail merely because a healthy
        // endpoint returns HTTP 4xx/5xx (for example, www.gstatic.com/
        // commonly returns 404). Curl still fails on DNS, connect, and TLS.
        "--noproxy",
        "",
        "-x",
        DNS_TEST_PROXY,
        "-sS",
        "--max-time",
        "6",
        "-o",
        "/dev/null",
        "-w",
        "domain=%{url_effective}\nprobe_path=magicnet-mixed\nhttp_code=%{http_code}\nproxy_ip=%{remote_ip}\ntime_total=%{time_total}\n",
        url,
    ]
}

fn dns_status(app: &App) {
    let profile = dns_profile(app);
    let bootstrap = dns_bootstrap(app);
    let via_proxy = dns_via_proxy(app, &profile);
    println!("profile={profile}");
    println!("via_proxy={}", if via_proxy { "true" } else { "false" });
    println!("bootstrap={bootstrap}");
    println!("bootstrap_transport={}", bootstrap_transport(&bootstrap));
    match profile.as_str() {
        "cloudflare-udp" | "cloudflare-udp-direct" => {
            println!("primary=1.1.1.1");
            println!("secondary=1.0.0.1");
            println!("transport=udp");
        }
        "cloudflare-dot" | "cloudflare-dot-direct" => {
            println!("primary=tls://1.1.1.1");
            println!("secondary=tls://1.0.0.1");
            println!("transport=dot");
        }
        "cloudflare-doh" | "cloudflare-doh-direct" => {
            println!("primary=https://cloudflare-dns.com/dns-query");
            println!("secondary=https://1.0.0.1/dns-query");
            println!("transport=doh");
        }
        "google-dot" | "google-dot-direct" => {
            println!("primary=tls://8.8.8.8");
            println!("secondary=tls://8.8.4.4");
            println!("transport=dot");
        }
        "google-doh" | "google-doh-direct" => {
            println!("primary=https://dns.google/dns-query");
            println!("secondary=https://8.8.4.4/dns-query");
            println!("transport=doh");
        }
        "adguard-doh" | "adguard-doh-direct" => {
            println!("primary=https://dns.adguard-dns.com/dns-query");
            println!("secondary=");
            println!("transport=doh");
        }
        "quad9-doh" | "quad9-doh-direct" => {
            println!("primary=https://dns.quad9.net/dns-query");
            println!("secondary=https://149.112.112.112/dns-query");
            println!("transport=doh");
        }
        _ => {
            println!("primary=bootstrap-local-dns");
            println!("transport=default");
        }
    }
}

fn dns_set(app: &App, profile: &str) -> Result<(), String> {
    let profile = normalize_profile(profile)?;
    update_dns_config(app, Some(profile), None)?;
    println!("[info] DNS profile set to {profile}");
    Ok(())
}

fn dns_bootstrap_cmd(app: &App, args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str).unwrap_or("status") {
        "status" => {
            let bootstrap = dns_bootstrap(app);
            println!("bootstrap={bootstrap}");
            println!("transport={}", bootstrap_transport(&bootstrap));
            Ok(())
        }
        "set" => {
            let bootstrap = normalize_bootstrap(args.get(1).map(String::as_str).unwrap_or_default())?;
            update_dns_config(app, None, Some(bootstrap))?;
            println!("[info] Bootstrap DNS set to {bootstrap}");
            Ok(())
        }
        _ => Err(dns_usage()),
    }
}

fn update_dns_config(
    app: &App,
    profile: Option<&str>,
    bootstrap: Option<&str>,
) -> Result<(), String> {
    let path = app.moddir.join(DNS_CONF);
    let previous = fs::read(&path).ok();
    let values = read_kv(path.clone());
    let profile = profile
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| dns_profile(app));
    let bootstrap = bootstrap
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| dns_bootstrap(app));
    let via_proxy = values
        .get("MAGICNET_DNS_VIA_PROXY")
        .map(String::as_str)
        .filter(|value| matches!(*value, "0" | "1"))
        .unwrap_or("1");
    let text = format!(
        "MAGICNET_DNS_PROFILE={profile}\nMAGICNET_BOOTSTRAP_DNS={bootstrap}\nMAGICNET_DNS_VIA_PROXY={via_proxy}\n"
    );
    write_text_file(app, Path::new(DNS_CONF), &text)?;
    if let Err(error) = run_magicnet_function(app, "magicnet_dns_apply") {
        restore_dns_config(app, previous.as_deref())?;
        let _ = run_magicnet_function(app, "magicnet_dns_apply");
        return Err(error);
    }
    if let Err(error) = restart_current_core(app) {
        restore_dns_config(app, previous.as_deref())?;
        let _ = run_magicnet_function(app, "magicnet_dns_apply");
        let _ = restart_current_core(app);
        let _ = run_magicnet_function(app, "magicnet_network_watch_sync");
        return Err(error);
    }
    if let Err(error) = run_magicnet_function(app, "magicnet_network_watch_sync") {
        restore_dns_config(app, previous.as_deref())?;
        let _ = run_magicnet_function(app, "magicnet_dns_apply");
        let _ = restart_current_core(app);
        let _ = run_magicnet_function(app, "magicnet_network_watch_sync");
        return Err(error);
    }
    Ok(())
}

fn restore_dns_config(app: &App, previous: Option<&[u8]>) -> Result<(), String> {
    match previous {
        Some(bytes) => write_text_file(app, Path::new(DNS_CONF), &String::from_utf8_lossy(bytes)),
        None => match fs::remove_file(app.moddir.join(DNS_CONF)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("restore DNS configuration: {error}")),
        },
    }
}

fn normalize_profile(profile: &str) -> Result<&'static str, String> {
    match profile {
        "default" | "system" | "local" => Ok("default"),
        // Cloudflare profiles - preserve -direct suffix
        "cloudflare" | "cloudflare-doh" | "1.1.1.1-doh" | "doh" => Ok("cloudflare-doh"),
        "cloudflare-doh-direct" => Ok("cloudflare-doh-direct"),
        "cloudflare-dot" | "1.1.1.1-dot" | "dot" => Ok("cloudflare-dot"),
        "cloudflare-dot-direct" => Ok("cloudflare-dot-direct"),
        "cloudflare-udp" | "1.1.1.1" | "udp" => Ok("cloudflare-udp"),
        "cloudflare-udp-direct" => Ok("cloudflare-udp-direct"),
        // Google profiles
        "google" | "google-doh" | "8.8.8.8-doh" => Ok("google-doh"),
        "google-doh-direct" => Ok("google-doh-direct"),
        "google-dot" | "8.8.8.8-dot" => Ok("google-dot"),
        "google-dot-direct" => Ok("google-dot-direct"),
        // AdGuard profiles
        "adguard" | "adguard-doh" | "94.140.14.14-doh" => Ok("adguard-doh"),
        "adguard-doh-direct" => Ok("adguard-doh-direct"),
        // Quad9 profiles
        "quad9" | "quad9-doh" | "9.9.9.9-doh" => Ok("quad9-doh"),
        "quad9-doh-direct" => Ok("quad9-doh-direct"),
        _ => Err(dns_usage()),
    }
}

fn normalize_bootstrap(bootstrap: &str) -> Result<&'static str, String> {
    match bootstrap {
        "system" | "android" | "android-system" => Ok("system"),
        "aliyun" | "alidns" | "ali" => Ok("aliyun"),
        "baidu" | "baidudns" => Ok("baidu"),
        "tencent" | "dnspod" | "doh.pub" => Ok("tencent"),
        _ => Err(dns_usage()),
    }
}

fn normalize_test_domain(domain: &str) -> Result<&str, String> {
    let domain = domain.trim().trim_end_matches('.');
    if domain.is_empty() || domain.len() > 253 {
        return Err("Usage: cli dns test [domain]".to_string());
    }
    let valid = domain.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    });
    if valid && domain.contains('.') {
        Ok(domain)
    } else {
        Err("Usage: cli dns test [domain]".to_string())
    }
}

fn dns_profile(app: &App) -> String {
    read_kv(app.moddir.join(DNS_CONF))
        .remove("MAGICNET_DNS_PROFILE")
        .and_then(|value| normalize_profile(&value).ok().map(ToOwned::to_owned))
        .unwrap_or_else(|| "default".to_string())
}

fn dns_bootstrap(app: &App) -> String {
    read_kv(app.moddir.join(DNS_CONF))
        .remove("MAGICNET_BOOTSTRAP_DNS")
        .and_then(|value| normalize_bootstrap(&value).ok().map(ToOwned::to_owned))
        .unwrap_or_else(|| "aliyun".to_string())
}

fn dns_via_proxy(app: &App, profile: &str) -> bool {
    if profile.ends_with("-direct") {
        return false;
    }
    !matches!(
        read_kv(app.moddir.join(DNS_CONF))
            .remove("MAGICNET_DNS_VIA_PROXY")
            .as_deref(),
        Some("0")
    )
}

fn bootstrap_transport(bootstrap: &str) -> &'static str {
    match bootstrap {
        "system" | "baidu" => "udp",
        _ => "doh",
    }
}

fn dns_usage() -> String {
    "Usage: cli dns {status|set <profile>|bootstrap {status|set <system|aliyun|baidu|tencent>}|test [domain]|apply}\n\
     Available profiles:\n\
     - default\n\
     - cloudflare-doh [direct: cloudflare-doh-direct]\n\
     - cloudflare-dot [direct: cloudflare-dot-direct]\n\
     - cloudflare-udp [direct: cloudflare-udp-direct]\n\
     - google-doh [direct: google-doh-direct]\n\
     - google-dot [direct: google-dot-direct]\n\
     - adguard-doh [direct: adguard-doh-direct]\n\
     - quad9-doh [direct: quad9-doh-direct]\n\
     Bootstrap DNS: system, aliyun, baidu, tencent"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{dns_test_curl_args, normalize_bootstrap, normalize_test_domain, DNS_TEST_PROXY};

    #[test]
    fn dns_test_domain_rejects_urls_and_shell_fragments() {
        assert_eq!(normalize_test_domain("example.com").unwrap(), "example.com");
        assert!(normalize_test_domain("https://example.com").is_err());
        assert!(normalize_test_domain("example.com;id").is_err());
        assert!(normalize_test_domain("localhost").is_err());
    }

    #[test]
    fn dns_test_curl_uses_local_proxy_for_root_callers() {
        let args = dns_test_curl_args("https://www.gstatic.com/");
        assert!(args.windows(2).any(|pair| pair == ["--noproxy", ""]));
        assert!(args.windows(2).any(|pair| pair == ["-x", DNS_TEST_PROXY]));
        let write_out = args
            .windows(2)
            .find(|pair| pair[0] == "-w")
            .map(|pair| pair[1])
            .expect("curl write-out format");
        assert!(write_out.contains("\nprobe_path=magicnet-mixed\n"));
        assert!(write_out.contains("\nproxy_ip=%{remote_ip}\n"));
        assert!(!write_out.contains("\nremote_ip="));
    }

    #[test]
    fn bootstrap_aliases_normalize_to_stable_tokens() {
        assert_eq!(normalize_bootstrap("android").unwrap(), "system");
        assert_eq!(normalize_bootstrap("alidns").unwrap(), "aliyun");
        assert_eq!(normalize_bootstrap("baidudns").unwrap(), "baidu");
        assert_eq!(normalize_bootstrap("dnspod").unwrap(), "tencent");
        assert!(normalize_bootstrap("cloudflare").is_err());
    }
}
