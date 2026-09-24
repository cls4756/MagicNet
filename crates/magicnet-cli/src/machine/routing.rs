use std::collections::{HashMap, HashSet};

use serde_json::{json, Map, Value};

use super::{envelope, MachineError, SINGBOX_CONFIG};
use crate::utils::read_json_file_bounded;
use crate::webui_api::curl_get_json;
use crate::App;

const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RULES: usize = 256;
const MAX_OUTBOUNDS: usize = 512;
const MAX_MEMBERS: usize = 256;
const MAX_CONDITION_VALUES: usize = 32;
const MAX_TEXT_LENGTH: usize = 256;
const INSPECTION_ERROR: MachineError = MachineError {
    code: "machine.routing_observation_failed",
    message: "unable to inspect the active routing configuration",
};
const INTERNAL_TAGS: &[&str] = &[
    "proxy-auto", "proxy", "select", "lan", "hotspot", "ad-block", "ad-allow", "cn-direct",
    "chain", "chain-hop1", "chain-exit", "chain-auto", "apple-cn", "microsoft-cn", "google-cn",
    "icloud", "bing", "dns-guard", "network-test", "ai-proxy", "ai-chatgpt", "ai-chatgpt-auto",
    "ai-gemini", "ai-gemini-auto", "ai-grok", "ai-grok-auto", "ai-claude", "ai-claude-auto",
    "proxy-rule", "dev-proxy", "social-proxy", "media-proxy", "game-proxy", "telegram-proxy",
    "google-proxy", "youtube-proxy", "github-proxy", "discord-proxy", "netflix-proxy",
    "spotify-proxy", "twitter-proxy", "whatsapp-proxy", "download-direct", "final", "direct", "block",
];
const CONDITION_KEYS: &[&str] = &[
    "domain", "domain_suffix", "domain_keyword", "domain_regex", "ip_cidr", "ip_is_private",
    "source_ip_cidr", "source_ip_is_private", "port", "port_range", "source_port",
    "source_port_range", "protocol", "network", "inbound", "process_name", "process_path",
    "package_name", "rule_set", "clash_mode", "query_type", "network_type",
];

pub(super) fn inspect(app: &App) -> Result<Value, MachineError> {
    let config = read_json_file_bounded(&app.moddir.join(SINGBOX_CONFIG), MAX_CONFIG_BYTES)
        .ok_or(INSPECTION_ERROR)?;
    let runtime = curl_get_json(app, "/proxies").ok();
    Ok(envelope(
        "routing.inspect",
        inspect_config(&config, runtime.as_ref()),
    ))
}

fn inspect_config(config: &Value, runtime: Option<&Value>) -> Value {
    let configured_outbounds = config
        .get("outbounds")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let runtime_proxies = runtime
        .and_then(|value| value.get("proxies"))
        .and_then(Value::as_object);
    let runtime_selections: HashMap<String, String> = runtime_proxies
        .into_iter()
        .flat_map(|proxies| proxies.iter())
        .filter_map(|(tag, value)| {
            safe_text(value.get("now")?, MAX_TEXT_LENGTH).map(|selected| (tag.clone(), selected))
        })
        .collect();
    let outbounds: Vec<&Value> = configured_outbounds
        .iter()
        .take(MAX_OUTBOUNDS)
        .collect();
    let tags: HashSet<&str> = outbounds
        .iter()
        .filter_map(|outbound| outbound.get("tag").and_then(Value::as_str))
        .collect();
    let configured_rules = config
        .get("route")
        .and_then(|route| route.get("rules"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let rules: Vec<Value> = configured_rules
        .iter()
        .take(MAX_RULES)
        .enumerate()
        .map(|(index, rule)| inspect_rule(index + 1, rule))
        .collect();
    let final_outbound = config
        .get("route")
        .and_then(|route| route.get("final"))
        .and_then(|value| safe_text(value, MAX_TEXT_LENGTH));
    let mut references: HashMap<String, Vec<Value>> = HashMap::new();
    for rule in &rules {
        if let Some(tag) = rule.get("outbound").and_then(Value::as_str) {
            references.entry(tag.to_string()).or_default().push(json!({
                "kind": "route_rule",
                "order": rule["order"],
            }));
        }
    }
    for outbound in &outbounds {
        let Some(parent) = outbound.get("tag").and_then(Value::as_str) else {
            continue;
        };
        for member in string_members(outbound.get("outbounds")) {
            if tags.contains(member.as_str()) {
                references.entry(member).or_default().push(json!({
                    "kind": "outbound_member",
                    "tag": parent,
                }));
            }
        }
    }
    if let Some(tag) = final_outbound.as_deref() {
        references.entry(tag.to_string()).or_default().push(json!({
            "kind": "route_final",
        }));
    }
    let outbound_values: Vec<Value> = outbounds
        .iter()
        .filter_map(|outbound| {
            let tag = safe_text(outbound.get("tag")?, MAX_TEXT_LENGTH)?;
            let type_name = safe_text(outbound.get("type")?, 64).unwrap_or_else(|| "unknown".into());
            let member_tags = string_members(outbound.get("outbounds"));
            let outbound_references = references.remove(&tag).unwrap_or_default();
            let reference_count = outbound_references.len();
            let internal = INTERNAL_TAGS.contains(&tag.as_str()) || tag.starts_with("magicnet-chain-");
            let mut value = json!({
                "tag": tag,
                "type": type_name,
                "source": if internal { "magicnet" } else { "configured" },
                "internal": internal,
                "members": member_tags,
                "references": outbound_references,
                "reference_count": reference_count,
            });
            if let Some(default) = outbound.get("default").and_then(|item| safe_text(item, MAX_TEXT_LENGTH)) {
                value["default"] = json!(default);
            }
            if let Some(selected) = runtime_selections.get(&tag) {
                value["selected"] = json!(selected);
            } else {
                value["selected"] = Value::Null;
            }
            Some(value)
        })
        .collect();
    json!({
        "rules": rules,
        "outbounds": outbound_values,
        "final_outbound": final_outbound,
        "runtime_state": if runtime.is_some() { "observed" } else { "unknown" },
        "truncated": configured_rules.len() > MAX_RULES || configured_outbounds.len() > MAX_OUTBOUNDS,
    })
}

fn inspect_rule(order: usize, rule: &Value) -> Value {
    let mut conditions = Map::new();
    for key in CONDITION_KEYS {
        let Some(value) = rule.get(*key) else {
            continue;
        };
        let mut value = safe_condition(value);
        if *key == "domain_suffix" {
            if let Some(values) = value.as_array_mut() {
                values.retain(|item| item.as_str() != Some("__magicnet_route__"));
            }
        }
        if !value.is_null() && value.as_array().is_none_or(|values| !values.is_empty()) {
            conditions.insert((*key).to_string(), value);
        }
    }
    let managed_domains: Vec<String> = rule
        .get("domain_suffix")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|value| value.as_str() != Some("__magicnet_route__"))
        .filter_map(|value| safe_text(value, MAX_TEXT_LENGTH))
        .take(MAX_CONDITION_VALUES)
        .collect();
    let managed = rule
        .get("domain_suffix")
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().any(|value| value.as_str() == Some("__magicnet_route__")));
    let outbound = rule
        .get("outbound")
        .and_then(|value| safe_text(value, MAX_TEXT_LENGTH));
    let managed_target = if managed {
        outbound.as_deref().and_then(|target| match target {
            "proxy-rule" => Some("proxy"),
            "direct" | "block" | "warp" => Some(target),
            _ => None,
        })
    } else {
        None
    };
    json!({
        "order": order,
        "conditions": conditions,
        "action": rule.get("action").and_then(|value| safe_text(value, 64)),
        "outbound": outbound,
        "source": if managed { "magicnet_custom_domain" } else { "effective_config" },
        "managed_domains": managed_domains,
        "managed_target": managed_target,
    })
}

fn safe_condition(value: &Value) -> Value {
    match value {
        Value::String(_) => safe_text(value, MAX_TEXT_LENGTH).map_or(Value::Null, Value::String),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .take(MAX_CONDITION_VALUES)
                .filter_map(|value| match value {
                    Value::String(_) => safe_text(value, MAX_TEXT_LENGTH).map(Value::String),
                    Value::Number(_) | Value::Bool(_) => Some(value.clone()),
                    _ => None,
                })
                .collect(),
        ),
        Value::Number(_) | Value::Bool(_) => value.clone(),
        _ => Value::Null,
    }
}

fn safe_text(value: &Value, limit: usize) -> Option<String> {
    value.as_str().map(|text| {
        text.chars()
            .filter(|character| !character.is_control())
            .take(limit)
            .collect::<String>()
    }).filter(|text| !text.is_empty())
}

fn string_members(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_MEMBERS)
        .filter_map(|value| safe_text(value, MAX_TEXT_LENGTH))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::inspect_config;
    use serde_json::json;

    #[test]
    fn inspection_exposes_order_targets_references_and_runtime_without_secrets() {
        let config = json!({
            "outbounds": [
                {"type": "selector", "tag": "proxy", "outbounds": ["node-a", "proxy-auto"], "default": "node-a"},
                {"type": "urltest", "tag": "proxy-auto", "outbounds": ["node-a"]},
                {"type": "shadowsocks", "tag": "node-a", "server": "ignored.example", "password": "test-only-secret"},
                {"type": "direct", "tag": "direct"},
            ],
            "route": {
                "rules": [
                    {"domain_suffix": ["__magicnet_route__", "example.com"], "outbound": "proxy-rule"},
                    {"package_name": ["com.example.app"], "outbound": "proxy"}
                ],
                "final": "final"
            }
        });
        let runtime = json!({"proxies": {"proxy": {"now": "node-a"}, "proxy-auto": {"now": "node-a"}}});
        let inspected = inspect_config(&config, Some(&runtime));
        assert_eq!(inspected["runtime_state"], "observed");
        assert_eq!(inspected["rules"][0]["order"], 1);
        assert_eq!(inspected["rules"][0]["source"], "magicnet_custom_domain");
        assert_eq!(inspected["rules"][0]["managed_target"], "proxy");
        assert_eq!(inspected["rules"][0]["managed_domains"][0], "example.com");
        assert_eq!(inspected["rules"][1]["conditions"]["package_name"][0], "com.example.app");
        assert_eq!(inspected["outbounds"][0]["selected"], "node-a");
        assert_eq!(inspected["outbounds"][0]["reference_count"], 2);
        let serialized = serde_json::to_string(&inspected).unwrap();
        assert!(!serialized.contains("test-only-secret"));
        assert!(!serialized.contains("ignored.example"));
    }

    #[test]
    fn unavailable_runtime_is_unknown_and_never_fabricated_as_selected() {
        let config = json!({
            "outbounds": [{"type": "selector", "tag": "proxy", "outbounds": ["node-a"]}],
            "route": {"rules": [], "final": "proxy"}
        });
        let inspected = inspect_config(&config, None);
        assert_eq!(inspected["runtime_state"], "unknown");
        assert!(inspected["outbounds"][0]["selected"].is_null());
    }
}
