use std::fs;
use std::io::Read;
use std::path::Path;

use serde_json::Value;

use crate::service::restart_current_core;
use crate::utils::read_json_file_bounded;
use crate::{read_kv, run_magicnet_function, write_kv, App};

const DOMAIN_FORWARD_CONF: &str = ".config/magicnet/domain-forward.conf";
const SINGBOX_CONFIG: &str = ".config/sing-box/config.json";
const SINGBOX_BINARY: &str = "bin/sing-box";
const ENABLE_KEY: &str = "MAGICNET_DOMAIN_FORWARD";

// Domain forwarding needs a fork change to sing-box: the sniff route action has
// to replace the packet destination with the sniffed domain before the outbound
// dials. The marker below is that option's JSON struct tag, and struct tags are
// part of the compiled type metadata, so a bounded scan of the installed core
// answers "does this build understand the key" without starting sing-box.
//
// The `json:"` prefix is what keeps the scan honest. Upstream still declares
// `sniff_override_destination` on the inbound options, so matching the bare field
// name reports every stock core as capable and the runtime then publishes a key
// the core refuses to decode.
const CORE_CAPABILITY_MARKER: &[u8] = b"json:\"override_destination";
const CORE_SCAN_LIMIT: u64 = 256 * 1024 * 1024;
const CORE_SCAN_CHUNK: usize = 1 << 20;

/// Observation of the domain-forwarding feature. `configured` records user
/// intent, `core_support` records whether the installed core can honour it, and
/// `effective` records what the active sing-box configuration actually carries.
/// They are reported separately so an unsupported core is never presented as a
/// working feature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Snapshot {
    pub(crate) configured: &'static str,
    pub(crate) core_support: &'static str,
    pub(crate) effective: &'static str,
    pub(crate) tcp_rule: bool,
}

pub(crate) fn snapshot(app: &App) -> Snapshot {
    let configured = if configured_enabled(app) {
        "enabled"
    } else {
        "disabled"
    };
    let core_support = if core_supports_override(app) {
        "available"
    } else {
        "unavailable"
    };
    let tcp_rule = tcp_override_rule_present(
        read_json_file_bounded(&app.moddir.join(SINGBOX_CONFIG), 4 * 1024 * 1024).as_ref(),
    );
    let effective = if configured == "disabled" {
        "disabled"
    } else if tcp_rule {
        "enabled"
    } else if core_support == "unavailable" {
        "unsupported"
    } else {
        "pending"
    };
    Snapshot {
        configured,
        core_support,
        effective,
        tcp_rule,
    }
}

pub(crate) fn domain_forward_cmd(app: &App, args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str).unwrap_or("status") {
        "status" => {
            print_status(&snapshot(app));
            Ok(())
        }
        "enable" => set_enabled(app, true),
        "disable" => set_enabled(app, false),
        _ => Err(usage()),
    }
}

fn set_enabled(app: &App, enabled: bool) -> Result<(), String> {
    write_kv(
        app,
        Path::new(DOMAIN_FORWARD_CONF),
        &[(ENABLE_KEY, if enabled { "1" } else { "0" }.to_string())],
    )?;
    // The rule lives in the generated sing-box configuration and is read at
    // process start, so the file write alone leaves the running core on the
    // previous behaviour.
    run_magicnet_function(app, "magicnet_transparent_apply")?;
    restart_current_core(app)?;

    let status = snapshot(app);
    print_status(&status);
    if enabled && status.effective != "enabled" {
        println!(
            "[info] domain forwarding is on, but not effective yet: core_support={} effective={}",
            status.core_support, status.effective
        );
    }
    Ok(())
}

fn print_status(status: &Snapshot) {
    println!("configured={}", status.configured);
    println!("core_support={}", status.core_support);
    println!("effective={}", status.effective);
    println!("tcp_only=1");
    if status.tcp_rule {
        println!("tcp_rule=present");
    } else {
        println!("tcp_rule=absent");
    }
}

fn usage() -> String {
    "Usage: cli domain-forward {status|enable|disable}".to_string()
}

/// Missing configuration means "on": the feature is a fix rather than an
/// opt-in experiment, so a fresh install should already forward domains. Only an
/// explicit negated value turns it off.
fn configured_enabled(app: &App) -> bool {
    let values = read_kv(app.moddir.join(DOMAIN_FORWARD_CONF));
    match values
        .get(ENABLE_KEY)
        .map(|value| value.trim().to_ascii_lowercase())
    {
        Some(value) => !matches!(value.as_str(), "0" | "false" | "off" | "no" | "disabled"),
        None => true,
    }
}

fn core_supports_override(app: &App) -> bool {
    let path = app.moddir.join(SINGBOX_BINARY);
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return false;
    };
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > CORE_SCAN_LIMIT {
        return false;
    }
    let Ok(mut file) = fs::File::open(&path) else {
        return false;
    };
    let mut chunk = vec![0u8; CORE_SCAN_CHUNK];
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let read = match file.read(&mut chunk) {
            Ok(0) => return false,
            Ok(read) => read,
            Err(_) => return false,
        };
        let mut window = carry;
        window.extend_from_slice(&chunk[..read]);
        if window
            .windows(CORE_CAPABILITY_MARKER.len())
            .any(|candidate| candidate == CORE_CAPABILITY_MARKER)
        {
            return true;
        }
        let keep = (CORE_CAPABILITY_MARKER.len() - 1).min(window.len());
        carry = window[window.len() - keep..].to_vec();
    }
}

fn tcp_override_rule_present(config: Option<&Value>) -> bool {
    config
        .and_then(|config| config.get("route"))
        .and_then(|route| route.get("rules"))
        .and_then(Value::as_array)
        .is_some_and(|rules| rules.iter().any(is_tcp_override_rule))
}

fn is_tcp_override_rule(rule: &Value) -> bool {
    rule.get("action").and_then(Value::as_str) == Some("sniff")
        && rule.get("override_destination").and_then(Value::as_bool) == Some(true)
        && rule_covers_tcp(rule)
}

fn rule_covers_tcp(rule: &Value) -> bool {
    match rule.get("network") {
        None | Some(Value::Null) => true,
        Some(Value::String(value)) => value == "tcp",
        Some(Value::Array(values)) => values.iter().any(|value| value.as_str() == Some("tcp")),
        Some(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::temp_app;
    use serde_json::json;

    fn write_config(app: &App, text: &str) {
        fs::create_dir_all(app.moddir.join(".config/sing-box")).expect("create config directory");
        fs::write(app.moddir.join(DOMAIN_FORWARD_CONF), text).expect("write domain forward config");
    }

    #[test]
    fn missing_configuration_keeps_domain_forwarding_on() {
        let app = temp_app();
        assert!(configured_enabled(&app));

        write_config(&app, "MAGICNET_DOMAIN_FORWARD=1\n");
        assert!(configured_enabled(&app));

        for value in ["0", "false", "OFF", "no", "disabled", "  0  "] {
            write_config(&app, &format!("MAGICNET_DOMAIN_FORWARD={value}\n"));
            assert!(!configured_enabled(&app), "{value:?} must disable");
        }
    }

    #[test]
    fn override_rule_detection_requires_a_tcp_capable_sniff_action() {
        let present = |value: Value| tcp_override_rule_present(Some(&value));
        assert!(present(json!({"route": {"rules": [
            {"inbound": ["tun-in"], "network": ["tcp"], "action": "sniff", "override_destination": true}
        ]}})));
        assert!(present(json!({"route": {"rules": [
            {"network": "tcp", "action": "sniff", "override_destination": true}
        ]}})));
        assert!(!present(json!({"route": {"rules": [
            {"network": ["udp"], "action": "sniff", "override_destination": true}
        ]}})));
        assert!(!present(json!({"route": {"rules": [
            {"network": ["tcp"], "action": "sniff"}
        ]}})));
        assert!(!present(json!({"route": {"rules": [
            {"network": ["tcp"], "action": "sniff", "override_destination": false}
        ]}})));
        assert!(!present(json!({"route": {"rules": [{"action": "sniff"}]}})));
        assert!(!tcp_override_rule_present(None));
    }

    #[test]
    fn capability_scan_finds_the_marker_across_chunk_boundaries() {
        let app = temp_app();
        assert!(!core_supports_override(&app));

        let bin = app.moddir.join(SINGBOX_BINARY);
        fs::create_dir_all(bin.parent().expect("binary parent")).expect("create bin directory");
        let mut payload = vec![b'a'; CORE_SCAN_CHUNK - 4];
        payload.extend_from_slice(CORE_CAPABILITY_MARKER);
        payload.extend_from_slice(&[b'b'; 16]);
        fs::write(&bin, &payload).expect("write fake core");
        assert!(core_supports_override(&app));

        fs::write(&bin, vec![b'a'; CORE_SCAN_CHUNK + 16]).expect("write core without marker");
        assert!(!core_supports_override(&app));
    }

    #[test]
    fn capability_scan_ignores_the_upstream_sniff_override_option() {
        // A stock sing-box still declares `sniff_override_destination` on its
        // inbound options. Matching that string reported an unpatched core as
        // capable, and the runtime then published a key the core cannot decode.
        let app = temp_app();
        let bin = app.moddir.join(SINGBOX_BINARY);
        fs::create_dir_all(bin.parent().expect("binary parent")).expect("create bin directory");
        fs::write(&bin, r#"json:"sniff_override_destination,omitempty""#)
            .expect("write stock core");
        assert!(!core_supports_override(&app));
    }

    #[test]
    fn effective_state_never_claims_success_for_an_unsupported_core() {
        let app = temp_app();
        assert_eq!(
            snapshot(&app),
            Snapshot {
                configured: "enabled",
                core_support: "unavailable",
                effective: "unsupported",
                tcp_rule: false,
            }
        );

        fs::create_dir_all(app.moddir.join("bin")).expect("create bin directory");
        fs::write(
            app.moddir.join(SINGBOX_BINARY),
            "json:\"override_destination",
        )
        .expect("write core");
        assert_eq!(snapshot(&app).effective, "pending");

        fs::create_dir_all(app.moddir.join(".config/sing-box")).expect("create config directory");
        fs::write(
            app.moddir.join(SINGBOX_CONFIG),
            r#"{"route":{"rules":[{"inbound":["tun-in"],"network":["tcp"],"action":"sniff","override_destination":true}]}}"#,
        )
        .expect("write sing-box config");
        let status = snapshot(&app);
        assert_eq!(status.effective, "enabled");
        assert!(status.tcp_rule);

        write_config(&app, "MAGICNET_DOMAIN_FORWARD=0\n");
        assert_eq!(snapshot(&app).effective, "disabled");
    }
}
