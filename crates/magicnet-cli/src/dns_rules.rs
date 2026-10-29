use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::{config_editor, replace_module_text_files_transactionally, service, App};

const CONFIG_PATH: &str = ".config/sing-box/config.json";
const RULES_PATH: &str = ".config/magicnet/dns-rules.json";
pub(crate) const PROFILE_SERVER: &str = "__magicnet_dns_profile__";
const BOOTSTRAP_SERVER: &str = "bootstrap-local-dns";
const LAN_SERVER: &str = "magicnet-lan-dns";
const MAX_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RULES: usize = 256;
const MAX_VALUES: usize = 128;
const MAX_TEXT: usize = 256;
const CONDITION_KEYS: &[&str] = &[
    "domain",
    "domain_suffix",
    "domain_keyword",
    "domain_regex",
    "rule_set",
    "query_type",
    "protocol",
    "network",
    "inbound",
    "ip_cidr",
    "source_ip_cidr",
    "port",
    "source_port",
    "ip_is_private",
    "source_ip_is_private",
    "invert",
];

struct Snapshot {
    config: Value,
    config_text: String,
    intent_text: Option<String>,
    rules: Vec<Value>,
    revision: String,
    application_server: String,
    servers: HashSet<String>,
    feature_servers: HashSet<String>,
    rule_sets: HashSet<String>,
}

pub(crate) fn inspect(app: &App) -> Result<Value, String> {
    let snapshot = load_snapshot(app)?;
    let rules: Vec<Value> = snapshot
        .rules
        .iter()
        .take(MAX_RULES)
        .enumerate()
        .map(|(index, rule)| {
            let reason = protected_reason(rule, &snapshot.feature_servers);
            let conditions = conditions_summary(rule, reason.is_some(), 0);
            json!({
                "id": index + 1,
                "conditions": conditions,
                "target": normalized_server(&safe_string(rule.get("server")).unwrap_or_default()),
                "editable": reason.is_none(),
                "protected_reason": reason,
            })
        })
        .collect();
    let mut resolvers = vec![
        json!({"tag": PROFILE_SERVER, "kind": "application", "available": snapshot.servers.contains(&snapshot.application_server)}),
        json!({"tag": BOOTSTRAP_SERVER, "kind": "bootstrap", "available": snapshot.servers.contains(BOOTSTRAP_SERVER)}),
    ];
    let mut custom_servers: Vec<&String> = snapshot
        .servers
        .iter()
        .filter(|tag| {
            tag.as_str() != BOOTSTRAP_SERVER
                && normalized_server(tag) != PROFILE_SERVER
                && !snapshot.feature_servers.contains(*tag)
        })
        .collect();
    custom_servers.sort();
    resolvers.extend(
        custom_servers
            .iter()
            .take(MAX_RULES)
            .map(|tag| json!({"tag": tag, "kind": "custom", "available": true})),
    );
    let mut rule_sets: Vec<&String> = snapshot.rule_sets.iter().collect();
    rule_sets.sort();
    Ok(json!({
        "revision": snapshot.revision,
        "source": if snapshot.intent_text.is_some() { "user_intent" } else { "configuration" },
        "rules": rules,
        "resolvers": resolvers,
        "rule_sets": rule_sets.iter().take(MAX_RULES).collect::<Vec<_>>(),
        "final_server": safe_string(snapshot.config.pointer("/dns/final")),
        "node_resolver": safe_string(snapshot.config.pointer("/route/default_domain_resolver"))
            .or_else(|| safe_string(snapshot.config.pointer("/route/default_domain_resolver/server"))),
        "application_server": snapshot.application_server,
        "truncated": snapshot.rules.len() > MAX_RULES || snapshot.servers.len() > MAX_RULES || snapshot.rule_sets.len() > MAX_RULES,
        "runtime_state": "unknown",
    }))
}

fn conditions_summary(rule: &Value, advanced: bool, depth: usize) -> Map<String, Value> {
    let mut conditions: Map<String, Value> = rule
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(key, value)| {
            (CONDITION_KEYS.contains(&key.as_str())
                || (advanced && ["clash_mode", "type", "mode", "action"].contains(&key.as_str())))
                && valid_condition(key, value)
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    if advanced && depth < 4 {
        if let Some(children) = rule.get("rules").and_then(Value::as_array) {
            conditions.insert(
                "rules".to_string(),
                Value::Array(
                    children
                        .iter()
                        .take(16)
                        .map(|child| Value::Object(conditions_summary(child, true, depth + 1)))
                        .collect(),
                ),
            );
        }
    }
    conditions
}

pub(crate) fn command(app: &App, args: &[String]) -> Result<(), String> {
    match args {
        [action, path] if action == "save-file" => save(app, Path::new(path)),
        _ => Err("Usage: cli dns rules save-file <webui-payload-path>".to_string()),
    }
}

fn save(app: &App, payload_path: &Path) -> Result<(), String> {
    if !inherited_config_lock(app) {
        return service::run_dns_rules_save(app, payload_path);
    }
    save_locked(app, payload_path)
}

pub(crate) fn inherited_config_lock(app: &App) -> bool {
    if std::env::var("MAGICNET_DNS_RULES_LOCKED").as_deref() != Ok("1")
        || std::env::var("MAGICNET_CONFIG_LOCK_HELD").as_deref() != Ok("1")
    {
        return false;
    }
    let parent = unsafe { libc::getppid() };
    let Ok(Some(owner)) = read_text(&app.moddir.join(".state/config.lock/pid")) else {
        return false;
    };
    let owner = owner.trim();
    if owner == parent.to_string() {
        return true;
    }
    let Ok(Some(stat)) = read_text(Path::new(&format!("/proc/{parent}/stat"))) else {
        return false;
    };
    crate::utils::proc_start_time(&stat).is_some_and(|start| owner == format!("{parent}:{start}"))
}

fn save_locked(app: &App, payload_path: &Path) -> Result<(), String> {
    let text = config_editor::read_webui_payload_text(app, payload_path)?;
    let payload: Value =
        serde_json::from_str(&text).map_err(|_| "DNS rules payload is not valid JSON")?;
    let snapshot = load_snapshot(app)?;
    let rules = validate_payload(&snapshot, &payload)?;
    let intent = format!(
        "{}\n",
        serde_json::to_string_pretty(&json!({"schema": 1, "rules": rules}))
            .map_err(|_| "unable to encode DNS rules")?
    );
    if intent.len() as u64 > MAX_BYTES {
        return Err("DNS rule intent exceeds its size limit".to_string());
    }
    let candidate = materialize(&snapshot, &rules)?;
    let candidate_text =
        serde_json::to_string(&candidate).map_err(|_| "unable to encode DNS configuration")?;
    service::apply_config_transaction_locked(
        app,
        || {
            if load_snapshot(app)?.revision != snapshot.revision {
                return Err(
                    "DNS configuration changed; reload the rules before applying".to_string(),
                );
            }
            config_editor::validate_singbox_config_text(app, &candidate_text)?;
            if load_snapshot(app)?.revision != snapshot.revision {
                return Err(
                    "DNS configuration changed during validation; reload the rules".to_string(),
                );
            }
            replace_module_text_files_transactionally(app, &[(Path::new(RULES_PATH), &intent)])
        },
        || {
            if let Some(previous) = snapshot.intent_text.as_deref() {
                replace_module_text_files_transactionally(
                    app,
                    &[
                        (Path::new(RULES_PATH), previous),
                        (Path::new(CONFIG_PATH), &snapshot.config_text),
                    ],
                )
            } else {
                replace_module_text_files_transactionally(
                    app,
                    &[(Path::new(CONFIG_PATH), &snapshot.config_text)],
                )?;
                match fs::remove_file(app.moddir.join(RULES_PATH)) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(_) => Err("unable to restore the previous DNS rule intent".to_string()),
                }
            }
        },
    )?;
    println!("[info] DNS rules validated, saved and applied");
    Ok(())
}

fn load_snapshot(app: &App) -> Result<Snapshot, String> {
    let config_text =
        read_text(&app.moddir.join(CONFIG_PATH))?.ok_or("DNS configuration is unavailable")?;
    let config: Value =
        serde_json::from_str(&config_text).map_err(|_| "DNS configuration is not valid JSON")?;
    let dns = config
        .get("dns")
        .and_then(Value::as_object)
        .ok_or("DNS configuration is unavailable")?;
    let configured_servers = dns
        .get("servers")
        .and_then(Value::as_array)
        .ok_or("DNS resolvers are unavailable")?;
    let servers: HashSet<String> = configured_servers
        .iter()
        .filter_map(|server| safe_string(server.get("tag")))
        .collect();
    let feature_servers: HashSet<String> = configured_servers
        .iter()
        .filter(|server| {
            server.get("type").and_then(Value::as_str) == Some("tailscale")
                || server.get("tag").and_then(Value::as_str) == Some(LAN_SERVER)
        })
        .filter_map(|server| safe_string(server.get("tag")))
        .collect();
    let configured_rules = dns
        .get("rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let intent_text = read_text(&app.moddir.join(RULES_PATH))?;
    let rules = if let Some(text) = intent_text.as_deref() {
        let intent: Value =
            serde_json::from_str(text).map_err(|_| "saved DNS rule intent is not valid JSON")?;
        if intent.get("schema").and_then(Value::as_u64) != Some(1) {
            return Err("saved DNS rule intent has an unsupported schema".to_string());
        }
        let mut rules: Vec<Value> = configured_rules
            .iter()
            .filter(|rule| feature_rule(rule, &feature_servers))
            .cloned()
            .collect();
        rules.extend(
            intent
                .get("rules")
                .and_then(Value::as_array)
                .ok_or("saved DNS rules are unavailable")?
                .clone(),
        );
        rules
    } else {
        configured_rules
    };
    if rules.iter().any(|rule| !rule.is_object()) {
        return Err("DNS rules must be JSON objects".to_string());
    }
    let mut digest = Sha256::new();
    digest.update(config_text.len().to_le_bytes());
    digest.update(config_text.as_bytes());
    digest.update([u8::from(intent_text.is_some())]);
    if let Some(text) = intent_text.as_deref() {
        digest.update(text.as_bytes());
    }
    let profile_text = read_text(&app.moddir.join(".config/magicnet/dns.conf"))?;
    digest.update(profile_text.as_deref().unwrap_or_default().as_bytes());
    let revision = format!("{:x}", digest.finalize());
    let profile = crate::dns::dns_profile(app);
    let application_server = if profile == "default" {
        BOOTSTRAP_SERVER.to_string()
    } else {
        format!(
            "{}-profile-dns",
            profile.split('-').next().unwrap_or("cloudflare")
        )
    };
    let rule_sets = config
        .pointer("/route/rule_set")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|rule| safe_string(rule.get("tag")))
        .collect();
    if read_text(&app.moddir.join(CONFIG_PATH))?.as_deref() != Some(config_text.as_str())
        || read_text(&app.moddir.join(RULES_PATH))? != intent_text
        || read_text(&app.moddir.join(".config/magicnet/dns.conf"))? != profile_text
    {
        return Err("DNS configuration changed during inspection; reload the rules".to_string());
    }
    Ok(Snapshot {
        config,
        config_text,
        intent_text,
        rules,
        revision,
        application_server,
        servers,
        feature_servers,
        rule_sets,
    })
}

fn read_text(path: &Path) -> Result<Option<String>, String> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("unable to read DNS configuration files".to_string()),
    };
    let metadata = file
        .metadata()
        .map_err(|_| "unable to inspect DNS configuration files")?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err("DNS configuration file exceeds its size or type limit".to_string());
    }
    let mut text = String::new();
    file.take(MAX_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| "unable to read DNS configuration files")?;
    if text.len() as u64 > MAX_BYTES {
        return Err("DNS configuration file exceeds its size limit".to_string());
    }
    Ok(Some(text))
}

fn validate_payload(snapshot: &Snapshot, payload: &Value) -> Result<Vec<Value>, String> {
    let object = payload
        .as_object()
        .ok_or("DNS rules payload must be an object")?;
    if object
        .keys()
        .any(|key| !["schema", "revision", "rules"].contains(&key.as_str()))
        || payload.get("schema").and_then(Value::as_u64) != Some(1)
    {
        return Err("DNS rules payload has an unsupported schema".to_string());
    }
    if payload.get("revision").and_then(Value::as_str) != Some(snapshot.revision.as_str()) {
        return Err("DNS configuration changed; reload the rules before applying".to_string());
    }
    let entries = payload
        .get("rules")
        .and_then(Value::as_array)
        .ok_or("DNS rules payload must contain a rule list")?;
    if entries.len() > MAX_RULES || snapshot.rules.len() > MAX_RULES {
        return Err("DNS rule count exceeds the editing limit".to_string());
    }
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let object = entry
            .as_object()
            .ok_or("DNS rule entries must be objects")?;
        if object
            .keys()
            .any(|key| !["id", "conditions", "target"].contains(&key.as_str()))
        {
            return Err(format!(
                "DNS rule {} contains unsupported fields",
                index + 1
            ));
        }
        let original = if let Some(id) = entry.get("id").filter(|value| !value.is_null()) {
            let id = id
                .as_u64()
                .filter(|id| *id > 0 && *id <= snapshot.rules.len() as u64)
                .ok_or("DNS rule identifier is invalid")? as usize;
            if !seen.insert(id) {
                return Err("DNS rule identifiers must not be duplicated".to_string());
            }
            Some((id, &snapshot.rules[id - 1]))
        } else {
            None
        };
        if let Some((id, rule)) = original {
            if protected_reason(rule, &snapshot.feature_servers).is_some() {
                if id != index + 1 || object.len() != 1 {
                    return Err(format!(
                        "DNS rule {id} is protected and cannot be changed or moved"
                    ));
                }
                result.push(rule.clone());
                continue;
            }
            if object.len() == 1 {
                let mut rule = rule.clone();
                if let Some(server) = rule.get("server").and_then(Value::as_str) {
                    rule["server"] = Value::String(normalized_server(server));
                }
                result.push(rule);
                continue;
            }
        }
        let conditions = entry
            .get("conditions")
            .and_then(Value::as_object)
            .ok_or("DNS rule conditions must be an object")?;
        if conditions.is_empty()
            || conditions.iter().any(|(key, value)| {
                !CONDITION_KEYS.contains(&key.as_str()) || !valid_condition(key, value)
            })
        {
            return Err(format!(
                "DNS rule {} has unsupported or invalid conditions",
                index + 1
            ));
        }
        if let Some(tags) = conditions.get("rule_set") {
            let members: Vec<&Value> = if let Some(values) = tags.as_array() {
                values.iter().collect()
            } else {
                vec![tags]
            };
            if members.iter().any(|value| {
                value
                    .as_str()
                    .is_none_or(|tag| !snapshot.rule_sets.contains(tag))
            }) {
                return Err(format!(
                    "DNS rule {} references an unavailable rule set",
                    index + 1
                ));
            }
        }
        let target = entry
            .get("target")
            .and_then(Value::as_str)
            .ok_or("DNS rule target is required")?;
        let resolved = if target == PROFILE_SERVER {
            snapshot.application_server.as_str()
        } else {
            target
        };
        if !snapshot.servers.contains(resolved) || snapshot.feature_servers.contains(resolved) {
            return Err(format!(
                "DNS rule {} references an unavailable or protected resolver",
                index + 1
            ));
        }
        let mut rule = conditions.clone();
        if let Some((_, original)) = original {
            for key in ["action", "type"] {
                if let Some(value) = original.get(key) {
                    rule.insert(key.to_string(), value.clone());
                }
            }
        }
        rule.insert("server".to_string(), Value::String(target.to_string()));
        result.push(Value::Object(rule));
    }
    for (index, rule) in snapshot.rules.iter().enumerate() {
        if protected_reason(rule, &snapshot.feature_servers).is_some()
            && !seen.contains(&(index + 1))
        {
            return Err(format!(
                "DNS rule {} is protected and cannot be removed",
                index + 1
            ));
        }
    }
    Ok(result
        .into_iter()
        .filter(|rule| !feature_rule(rule, &snapshot.feature_servers))
        .collect())
}

fn materialize(snapshot: &Snapshot, rules: &[Value]) -> Result<Value, String> {
    let mut config = snapshot.config.clone();
    let mut combined: Vec<Value> = snapshot
        .rules
        .iter()
        .filter(|rule| feature_rule(rule, &snapshot.feature_servers))
        .cloned()
        .collect();
    for rule in rules {
        let mut rule = rule.clone();
        if rule
            .get("server")
            .and_then(Value::as_str)
            .is_some_and(|server| normalized_server(server) == PROFILE_SERVER)
        {
            rule["server"] = Value::String(snapshot.application_server.clone());
        }
        combined.push(rule);
    }
    let dns = config
        .get_mut("dns")
        .and_then(Value::as_object_mut)
        .ok_or("DNS configuration is unavailable")?;
    dns.insert("rules".to_string(), Value::Array(combined));
    Ok(config)
}

fn protected_reason(rule: &Value, feature_servers: &HashSet<String>) -> Option<&'static str> {
    if rule.get("clash_mode").is_some() {
        return Some("mode");
    }
    if feature_rule(rule, feature_servers) {
        return Some("feature");
    }
    let Some(object) = rule.as_object() else {
        return Some("unsupported");
    };
    if safe_string(rule.get("server")).is_none()
        || !object
            .keys()
            .any(|key| CONDITION_KEYS.contains(&key.as_str()))
        || object.iter().any(|(key, value)| {
            key != "server"
                && !supported_metadata(key, value)
                && (!CONDITION_KEYS.contains(&key.as_str()) || !valid_condition(key, value))
        })
    {
        return Some("unsupported");
    }
    None
}

fn supported_metadata(key: &str, value: &Value) -> bool {
    (key == "action" && value.as_str() == Some("route"))
        || (key == "type" && value.as_str() == Some("default"))
}

fn feature_rule(rule: &Value, feature_servers: &HashSet<String>) -> bool {
    rule.get("server")
        .and_then(Value::as_str)
        .is_some_and(|server| feature_servers.contains(server))
}

fn normalized_server(server: &str) -> String {
    if server == PROFILE_SERVER
        || server == "default-remote-dns"
        || server == "doh-cloudflare"
        || server == "doh-google"
        || ["cloudflare", "google", "adguard", "quad9"]
            .iter()
            .any(|provider| {
                server == format!("{provider}-profile-dns")
                    || server == format!("{provider}-backup-dns")
            })
    {
        PROFILE_SERVER.to_string()
    } else {
        server.to_string()
    }
}

fn safe_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| {
            !text.is_empty() && text.len() <= MAX_TEXT && !text.chars().any(char::is_control)
        })
        .map(str::to_string)
}

fn valid_condition(key: &str, value: &Value) -> bool {
    if ["invert", "ip_is_private", "source_ip_is_private"].contains(&key) {
        return value.is_boolean();
    }
    let valid_member = |member: &Value| {
        if ["query_type", "port", "source_port"].contains(&key) && member.is_u64() {
            return member
                .as_u64()
                .is_some_and(|number| number <= 65535 && (key == "query_type" || number > 0));
        }
        safe_string(Some(member)).is_some()
    };
    if let Some(values) = value.as_array() {
        !values.is_empty() && values.len() <= MAX_VALUES && values.iter().all(valid_member)
    } else {
        valid_member(value)
    }
}

#[cfg(test)]
#[path = "../tests/internal/dns_rules.rs"]
mod tests;
