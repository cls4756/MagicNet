use std::fs::{self, OpenOptions};
use std::io::Read;
use std::net::{IpAddr, Ipv6Addr};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{config_editor, dns_rules, replace_module_text_files_transactionally, service, App};

pub(crate) const CUSTOM_PATH: &str = ".config/magicnet/dns-custom.json";
const DNS_PATH: &str = ".config/magicnet/dns.conf";
const CONFIG_PATH: &str = ".config/sing-box/config.json";
const RULES_PATH: &str = ".config/magicnet/dns-rules.json";
const CUSTOM_TAG: &str = "custom-profile-dns";
const INVALID_URL: &str = "custom DoH requires a valid HTTPS URL without credentials or fragments";

#[derive(Clone)]
pub(crate) struct CustomDoh {
    schema: u64,
    server: String,
    server_port: u16,
    path: String,
    pub(crate) via_proxy: bool,
}

impl CustomDoh {
    pub(crate) fn value(&self) -> Value {
        json!({"schema": self.schema, "server": self.server, "server_port": self.server_port,
            "path": self.path, "via_proxy": self.via_proxy})
    }

    fn url(&self) -> String {
        let host = if self.server.contains(':') {
            format!("[{}]", self.server)
        } else {
            self.server.clone()
        };
        let port = if self.server_port == 443 {
            String::new()
        } else {
            format!(":{}", self.server_port)
        };
        format!("https://{host}{port}{}", self.path)
    }

    fn server_value(&self, mark: Value) -> Value {
        let mut value = json!({
            "type": "https", "tag": CUSTOM_TAG, "server": self.server,
            "server_port": self.server_port, "path": self.path,
            "tls": {"server_name": self.server},
            "domain_resolver": {"server": "bootstrap-local-dns"}
        });
        if self.server.contains(':') && self.server_port == 443 {
            value["headers"] = json!({"Host": format!("[{}]", self.server)});
        }
        if self.via_proxy {
            value["detour"] = json!("proxy");
        } else {
            value["routing_mark"] = mark;
        }
        value
    }
}

fn parse_url(url: &str, via_proxy: bool) -> Result<CustomDoh, String> {
    if url.len() > 4096
        || !url.is_ascii()
        || url
            .bytes()
            .any(|byte| byte <= 32 || byte == 127 || b"\\\"<>`{}|^".contains(&byte))
    {
        return Err(INVALID_URL.to_string());
    }
    let rest = url.strip_prefix("https://").ok_or(INVALID_URL)?;
    if rest.contains('#') {
        return Err(INVALID_URL.to_string());
    }
    let split = rest.find(['/', '?']).unwrap_or(rest.len());
    let authority = &rest[..split];
    if authority.is_empty() || authority.contains('@') {
        return Err(INVALID_URL.to_string());
    }
    let (server, port) = if let Some(ipv6) = authority.strip_prefix('[') {
        let end = ipv6.find(']').ok_or(INVALID_URL)?;
        let server = ipv6[..end].parse::<Ipv6Addr>().map_err(|_| INVALID_URL)?;
        (server.to_string(), &ipv6[end + 1..])
    } else {
        let split = authority.find(':').unwrap_or(authority.len());
        let server = authority[..split]
            .trim_end_matches('.')
            .to_ascii_lowercase();
        if server.is_empty()
            || server.len() > 253
            || !server.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
        {
            return Err(INVALID_URL.to_string());
        }
        if server
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
            && server.parse::<IpAddr>().is_err()
        {
            return Err(INVALID_URL.to_string());
        }
        (server, &authority[split..])
    };
    let server_port = if port.is_empty() {
        443
    } else {
        let digits = port.strip_prefix(':').ok_or(INVALID_URL)?;
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(INVALID_URL.to_string());
        }
        digits.parse::<u16>().map_err(|_| INVALID_URL)?
    };
    if server_port == 0 {
        return Err(INVALID_URL.to_string());
    }
    let suffix = &rest[split..];
    let path = if suffix.is_empty() {
        "/dns-query".to_string()
    } else if suffix.starts_with('?') {
        format!("/dns-query{suffix}")
    } else {
        suffix.to_string()
    };
    if path.len() > 2048 {
        return Err(INVALID_URL.to_string());
    }
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let encoded = path.get(index + 1..index + 3).ok_or(INVALID_URL)?;
            let decoded = u8::from_str_radix(encoded, 16).map_err(|_| INVALID_URL)?;
            if decoded < 32 || decoded == 127 {
                return Err(INVALID_URL.to_string());
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    Ok(CustomDoh {
        schema: 1,
        server,
        server_port,
        path,
        via_proxy,
    })
}

pub(crate) fn validate_intent(text: &str) -> Result<CustomDoh, String> {
    if text.len() > 8192 {
        return Err("custom DoH configuration exceeds its size limit".to_string());
    }
    let value: Value =
        serde_json::from_str(text).map_err(|_| "invalid custom DoH configuration")?;
    let invalid = "invalid custom DoH configuration";
    if value.as_object().is_none_or(|object| {
        object.keys().any(|key| {
            !["schema", "server", "server_port", "path", "via_proxy"].contains(&key.as_str())
        })
    }) {
        return Err(invalid.to_string());
    }
    let config = CustomDoh {
        schema: value.get("schema").and_then(Value::as_u64).ok_or(invalid)?,
        server: value
            .get("server")
            .and_then(Value::as_str)
            .ok_or(invalid)?
            .to_string(),
        server_port: value
            .get("server_port")
            .and_then(Value::as_u64)
            .and_then(|port| u16::try_from(port).ok())
            .ok_or(invalid)?,
        path: value
            .get("path")
            .and_then(Value::as_str)
            .ok_or(invalid)?
            .to_string(),
        via_proxy: value
            .get("via_proxy")
            .and_then(Value::as_bool)
            .ok_or(invalid)?,
    };
    let normalized = parse_url(&config.url(), config.via_proxy)?;
    if config.schema != 1 || config.server != normalized.server || config.path != normalized.path {
        return Err("invalid custom DoH configuration".to_string());
    }
    Ok(config)
}

fn read_text(path: &Path, limit: u64) -> Result<Option<String>, String> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("unable to read custom DoH configuration".to_string()),
    };
    let metadata = file
        .metadata()
        .map_err(|_| "unable to inspect custom DoH configuration")?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err("custom DoH configuration exceeds its size or type limit".to_string());
    }
    let mut text = String::new();
    file.take(limit + 1)
        .read_to_string(&mut text)
        .map_err(|_| "unable to read custom DoH configuration")?;
    if text.len() as u64 > limit {
        return Err("custom DoH configuration exceeds its size limit".to_string());
    }
    Ok(Some(text))
}

pub(crate) fn load(app: &App) -> Result<Option<CustomDoh>, String> {
    read_text(&app.moddir.join(CUSTOM_PATH), 8192)?
        .map(|text| validate_intent(&text))
        .transpose()
}

struct Snapshot {
    files: Vec<(&'static str, Option<String>)>,
    revision: String,
}

fn snapshot(app: &App) -> Result<Snapshot, String> {
    let mut files = Vec::new();
    let mut digest = Sha256::new();
    for (path, limit) in [
        (DNS_PATH, 65536),
        (CUSTOM_PATH, 8192),
        (CONFIG_PATH, 4 * 1024 * 1024),
        (RULES_PATH, 4 * 1024 * 1024),
    ] {
        let text = read_text(&app.moddir.join(path), limit)?;
        digest.update([u8::from(text.is_some())]);
        digest.update(text.as_deref().unwrap_or_default().len().to_le_bytes());
        digest.update(text.as_deref().unwrap_or_default().as_bytes());
        files.push((path, text));
    }
    let revision = format!("{:x}", digest.finalize());
    Ok(Snapshot { files, revision })
}

pub(crate) fn inspect(app: &App) -> Result<Value, String> {
    let before = snapshot(app)?;
    let config = load(app)?;
    if before.revision != snapshot(app)?.revision {
        return Err("DNS configuration changed during inspection".to_string());
    }
    Ok(json!({
        "configured": config.is_some(), "url": config.as_ref().map(CustomDoh::url),
        "via_proxy": config.as_ref().map(|config| config.via_proxy).unwrap_or(true),
        "revision": before.revision
    }))
}

pub(crate) fn command(app: &App, args: &[String]) -> Result<(), String> {
    match args {
        [action, path] if action == "save-file" => {
            if dns_rules::inherited_config_lock(app) {
                save_locked(app, Path::new(path))
            } else {
                service::run_dns_custom_save(app, Path::new(path))
            }
        }
        _ => Err("Usage: cli dns custom save-file <webui-payload-path>".to_string()),
    }
}

fn dns_intent(previous: &str) -> String {
    let mut lines: Vec<&str> = previous
        .lines()
        .filter(|line| !line.trim_start().starts_with("MAGICNET_DNS_PROFILE="))
        .collect();
    lines.push("MAGICNET_DNS_PROFILE=custom-doh");
    format!("{}\n", lines.join("\n"))
}

fn candidate(text: &str, custom: &CustomDoh) -> Result<String, String> {
    let mut config: Value =
        serde_json::from_str(text).map_err(|_| "DNS configuration is not valid JSON")?;
    let dns = config
        .get_mut("dns")
        .and_then(Value::as_object_mut)
        .ok_or("DNS configuration is unavailable")?;
    let servers = dns
        .get_mut("servers")
        .and_then(Value::as_array_mut)
        .ok_or("DNS resolvers are unavailable")?;
    let mark = servers
        .iter()
        .find(|server| server["tag"] == "bootstrap-local-dns")
        .map(|server| {
            server
                .get("routing_mark")
                .cloned()
                .unwrap_or(json!(1073741824))
        })
        .ok_or("Bootstrap DNS is unavailable")?;
    servers.retain(|server| {
        server
            .get("tag")
            .and_then(Value::as_str)
            .is_none_or(|tag| dns_rules::normalized_server(tag) != dns_rules::PROFILE_SERVER)
    });
    servers.push(custom.server_value(mark));
    if let Some(rules) = dns.get_mut("rules").and_then(Value::as_array_mut) {
        for rule in rules {
            if rule
                .get("server")
                .and_then(Value::as_str)
                .is_some_and(|tag| dns_rules::normalized_server(tag) == dns_rules::PROFILE_SERVER)
            {
                rule["server"] = json!(CUSTOM_TAG);
            }
        }
    }
    dns.insert("final".to_string(), json!(CUSTOM_TAG));
    serde_json::to_string(&config)
        .map_err(|_| "unable to encode custom DoH configuration".to_string())
}

fn save_locked(app: &App, payload_path: &Path) -> Result<(), String> {
    let text = config_editor::read_webui_payload_text(app, payload_path)?;
    let payload: Value =
        serde_json::from_str(&text).map_err(|_| "custom DoH payload is not valid JSON")?;
    let before = snapshot(app)?;
    if payload.get("schema").and_then(Value::as_u64) != Some(1)
        || payload.as_object().is_none_or(|object| {
            object
                .keys()
                .any(|key| !["schema", "revision", "url", "via_proxy"].contains(&key.as_str()))
        })
    {
        return Err("custom DoH payload has an unsupported schema".to_string());
    }
    if payload.get("revision").and_then(Value::as_str) != Some(before.revision.as_str()) {
        return Err("DNS configuration changed; reload before applying custom DoH".to_string());
    }
    let custom = parse_url(
        payload
            .get("url")
            .and_then(Value::as_str)
            .ok_or(INVALID_URL)?,
        payload
            .get("via_proxy")
            .and_then(Value::as_bool)
            .ok_or("custom DoH route must be a boolean")?,
    )?;
    let intent = format!(
        "{}\n",
        serde_json::to_string_pretty(&custom.value())
            .map_err(|_| "unable to encode custom DoH intent")?
    );
    let dns_text = dns_intent(before.files[0].1.as_deref().unwrap_or_default());
    let candidate_text = candidate(
        before.files[2]
            .1
            .as_deref()
            .ok_or("DNS configuration is unavailable")?,
        &custom,
    )?;
    service::apply_config_transaction_locked(
        app,
        || {
            if snapshot(app)?.revision != before.revision {
                return Err("DNS configuration changed before validation".to_string());
            }
            config_editor::validate_singbox_config_text(app, &candidate_text)
                .map_err(|_| "custom DoH configuration failed native validation".to_string())?;
            if snapshot(app)?.revision != before.revision {
                return Err("DNS configuration changed during validation".to_string());
            }
            replace_module_text_files_transactionally(
                app,
                &[
                    (Path::new(DNS_PATH), &dns_text),
                    (Path::new(CUSTOM_PATH), &intent),
                ],
            )
        },
        || {
            let restore: Vec<(&Path, &str)> = before
                .files
                .iter()
                .take(3)
                .filter_map(|(path, text)| text.as_deref().map(|text| (Path::new(path), text)))
                .collect();
            replace_module_text_files_transactionally(app, &restore)?;
            for (path, text) in before.files.iter().take(2) {
                if text.is_none() {
                    fs::remove_file(app.moddir.join(path))
                        .map_err(|_| "unable to restore previous DNS intent")?;
                }
            }
            Ok(())
        },
    )
    .map_err(|_| "custom DoH application failed; check health before retrying".to_string())?;
    println!("[info] Custom DoH validated, saved and applied");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_https_and_preserves_endpoint_components() {
        for url in [
            "https://Resolver.Example:8443/private/dns?key=fixture",
            "https://[2001:db8::1]:5443/dns-query",
            "https://192.168.1.2/dns-query",
        ] {
            let parsed = parse_url(url, false).unwrap();
            let encoded = serde_json::to_string(&parsed.value()).unwrap();
            assert_eq!(validate_intent(&encoded).unwrap().url(), parsed.url());
            assert!(!parsed.via_proxy);
        }
        assert_eq!(
            parse_url("https://dns.example", true).unwrap().path,
            "/dns-query"
        );
        let ipv6 = parse_url("https://[2001:db8::1]/dns-query?q=a%20b", true).unwrap();
        assert_eq!(
            ipv6.server_value(json!(1073741824))["headers"]["Host"],
            "[2001:db8::1]"
        );
    }

    #[test]
    fn rejects_invalid_or_unsafe_urls_without_echoing_them() {
        for url in [
            "http://dns.example/dns-query",
            "https://user:secret@dns.example/dns-query",
            "https://dns.example/#secret",
            "https://dns.example:0/dns",
            "https://dns.example:65536/dns",
            "https://dns.example:/dns",
            "https://dns.example/%0D%0Asecret",
            "https://dns.example/%GG",
            "https://dns.example/a b",
            "https://dns.example\\private",
            "https://[invalid]/dns",
            "https://127.1/dns",
            "https:///dns",
        ] {
            assert_eq!(parse_url(url, true).err().as_deref(), Some(INVALID_URL));
        }
    }
}
