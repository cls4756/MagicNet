use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{config_editor, replace_module_text_files_transactionally, service, App};

const CONFIG: &str = ".config/sing-box/config.json";
const INTENT: &str = ".config/magicnet/routing-policy.json";
const MAX_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RULES: usize = 256;
const MAX_GROUPS: usize = 64;
const BUSINESS: &[&str] = &[
    "cn-direct",
    "apple-cn",
    "microsoft-cn",
    "google-cn",
    "icloud",
    "bing",
    "dns-guard",
    "network-test",
    "ai-proxy",
    "ai-chatgpt",
    "ai-gemini",
    "ai-grok",
    "ai-claude",
    "ai-chatgpt-auto",
    "ai-gemini-auto",
    "ai-grok-auto",
    "ai-claude-auto",
    "proxy-rule",
    "dev-proxy",
    "social-proxy",
    "media-proxy",
    "game-proxy",
    "telegram-proxy",
    "google-proxy",
    "youtube-proxy",
    "github-proxy",
    "discord-proxy",
    "netflix-proxy",
    "spotify-proxy",
    "twitter-proxy",
    "whatsapp-proxy",
    "download-direct",
];
const SYSTEM: &[&str] = &[
    "select",
    "lan",
    "hotspot",
    "ad-block",
    "ad-allow",
    "chain-hop1",
    "chain-exit",
    "chain-auto",
];
const MATCH_KEYS: &[&str] = &[
    "domain",
    "domain_suffix",
    "domain_keyword",
    "domain_regex",
    "ip_cidr",
    "source_ip_cidr",
    "port",
    "port_range",
    "source_port",
    "source_port_range",
    "protocol",
    "network",
    "process_name",
    "process_path",
    "package_name",
    "rule_set",
    "ip_version",
    "ip_is_private",
    "source_ip_is_private",
    "invert",
    "type",
    "mode",
    "rules",
];

struct Snapshot {
    config: Value,
    config_text: String,
    intent_text: Option<String>,
    policy: Value,
    revision: String,
}

fn read_text(path: &Path) -> Result<Option<String>, String> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("unable to read routing configuration".into()),
    };
    let metadata = file
        .metadata()
        .map_err(|_| "unable to inspect routing configuration")?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err("routing configuration exceeds its size or type limit".into());
    }
    let mut text = String::new();
    file.take(MAX_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| "unable to read routing configuration")?;
    if text.len() as u64 > MAX_BYTES {
        return Err("routing configuration exceeds its size limit".into());
    }
    Ok(Some(text))
}

fn encode(value: &Value) -> Result<String, String> {
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(value)
            .map_err(|_| "unable to encode routing configuration")?
    );
    if text.len() as u64 > MAX_BYTES {
        return Err("routing configuration exceeds its size limit".into());
    }
    Ok(text)
}

fn array(value: &Value, key: &str) -> Vec<Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn tag(value: &Value) -> &str {
    value.get("tag").and_then(Value::as_str).unwrap_or_default()
}

fn business(tag: &str) -> bool {
    BUSINESS.contains(&tag)
}

fn user_group(outbound: &Value) -> bool {
    matches!(
        outbound.get("type").and_then(Value::as_str),
        Some("selector" | "urltest")
    ) && !business(tag(outbound))
        && !SYSTEM.contains(&tag(outbound))
        && !["proxy", "proxy-auto", "final", "chain"].contains(&tag(outbound))
        && !tag(outbound).starts_with("magicnet-")
}

fn editable(rule: &Value) -> bool {
    let Some(object) = rule.as_object() else {
        return false;
    };
    let Some(target) = object.get("outbound").and_then(Value::as_str) else {
        return false;
    };
    !SYSTEM.contains(&target)
        && !target.starts_with("magicnet-")
        && !object.contains_key("action")
        && !object.contains_key("clash_mode")
        && object
            .keys()
            .all(|key| key == "outbound" || MATCH_KEYS.contains(&key.as_str()))
        && !(target == "block"
            && (object.get("protocol").and_then(Value::as_str) == Some("icmp")
                || object
                    .get("ip_cidr")
                    .and_then(Value::as_array)
                    .is_some_and(|values| {
                        values
                            .iter()
                            .any(|value| matches!(value.as_str(), Some("224.0.0.0/4" | "ff00::/8")))
                    })))
        && !object
            .get("domain_suffix")
            .and_then(Value::as_array)
            .is_some_and(|values| {
                values.iter().any(|value| {
                    value
                        .as_str()
                        .is_some_and(|value| value.starts_with("__magicnet_warp"))
                })
            })
        && !object
            .get("package_name")
            .and_then(Value::as_array)
            .is_some_and(|values| {
                values.iter().any(|value| {
                    value
                        .as_str()
                        .is_some_and(|value| value.starts_with("__magicnet_app_"))
                })
            })
}

fn mapped_target(target: &str) -> &str {
    match target {
        "cn-direct" | "apple-cn" | "microsoft-cn" | "google-cn" | "icloud" | "download-direct" => {
            "direct"
        }
        target if business(target) => "proxy-auto",
        target => target,
    }
}

fn remap(value: &mut Value) {
    if let Some(target) = value.get("outbound").and_then(Value::as_str) {
        value["outbound"] = json!(mapped_target(target));
    }
    if let Some(children) = value.get_mut("rules").and_then(Value::as_array_mut) {
        for child in children {
            remap(child);
        }
    }
}

fn default_policy(config: &Value) -> Value {
    let mut rules: Vec<Value> = Vec::new();
    let mut adjacent = false;
    for mut rule in array(&config["route"], "rules") {
        if !editable(&rule) {
            adjacent = false;
            continue;
        }
        remap(&mut rule);
        let mut conditions = rule.as_object().cloned().unwrap_or_default();
        let target = conditions.remove("outbound").unwrap_or(json!("proxy-auto"));
        if let Some(domains) = conditions
            .get_mut("domain_suffix")
            .and_then(Value::as_array_mut)
        {
            domains.retain(|item| item.as_str() != Some("__magicnet_route__"));
        }
        if conditions.is_empty()
            || conditions
                .get("domain_suffix")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
        {
            continue;
        }
        let next = json!({"conditions": conditions, "target": target});
        if let Some(previous) = rules.last_mut().filter(|_| adjacent) {
            let left = previous["conditions"].as_object().unwrap();
            let right = next["conditions"].as_object().unwrap();
            if previous["target"] == next["target"] && left.len() == 1 && right.len() == 1 {
                let key = left.keys().next().unwrap().clone();
                if ["rule_set", "domain", "domain_suffix", "package_name"].contains(&key.as_str())
                    && right.contains_key(&key)
                    && left[&key].is_array()
                    && right[&key].is_array()
                {
                    let values = previous["conditions"][&key].as_array_mut().unwrap();
                    for value in right[&key].as_array().unwrap() {
                        if !values.contains(value) {
                            values.push(value.clone());
                        }
                    }
                    continue;
                }
            }
        }
        rules.push(next);
        adjacent = true;
    }
    let groups: Vec<Value> = array(config, "outbounds").iter().filter(|outbound| user_group(outbound))
        .map(|outbound| {
            let mut group = json!({"tag": tag(outbound), "type": outbound["type"], "members": array(outbound, "outbounds")});
            if outbound["type"] == "urltest" {
                group["url"] = outbound.get("url").cloned().unwrap_or(json!("https://www.gstatic.com/generate_204"));
            } else {
                group["default"] = outbound.get("default").cloned().unwrap_or_else(|| group["members"][0].clone());
            }
            group
        }).collect();
    json!({"schema": 1, "rules": rules, "groups": groups, "final": "proxy-auto"})
}

fn saved_selections(app: &App) -> Result<HashMap<String, String>, String> {
    let text = match read_text(&app.moddir.join(".config/magicnet/selector-selections.json"))? {
        Some(text) => Some(text),
        None => read_text(&app.moddir.join(".state/sing-box/selector-selections.json"))?,
    };
    match text {
        Some(text) => {
            serde_json::from_str(&text).map_err(|_| "invalid saved selector choices".into())
        }
        None => Ok(HashMap::new()),
    }
}

fn default_policy_for_app(app: &App, config: &Value) -> Result<Value, String> {
    let selections = saved_selections(app)?;
    let mut migrated = config.clone();
    let original_outbounds = array(config, "outbounds");
    let mut restored = Vec::new();
    let mut names = HashMap::new();
    fn restore_group(
        name: &str,
        originals: &[Value],
        selections: &HashMap<String, String>,
        names: &mut HashMap<String, String>,
        restored: &mut Vec<Value>,
    ) -> String {
        if !business(name) {
            return name.to_string();
        }
        if let Some(mapped) = names.get(name) {
            return mapped.clone();
        }
        let mut next = format!("user-{name}");
        while originals
            .iter()
            .chain(restored.iter())
            .any(|outbound| tag(outbound) == next)
        {
            next.push('-');
        }
        names.insert(name.to_string(), next.clone());
        if let Some(original) = originals.iter().find(|outbound| tag(outbound) == name) {
            let mut group = original.clone();
            group["tag"] = json!(next);
            let mut members: Vec<Value> = array(original, "outbounds")
                .iter()
                .filter_map(Value::as_str)
                .map(|member| {
                    json!(restore_group(
                        member, originals, selections, names, restored
                    ))
                })
                .collect();
            if original["type"] == "selector" {
                if let Some(default) = selections
                    .get(name)
                    .map(String::as_str)
                    .or_else(|| original.get("default").and_then(Value::as_str))
                {
                    let default = json!(restore_group(
                        default, originals, selections, names, restored
                    ));
                    if !members.contains(&default) {
                        members.push(default.clone());
                    }
                    group["default"] = default;
                }
            }
            group["outbounds"] = json!(members);
            restored.push(group);
        }
        next
    }
    for rule in migrated["route"]["rules"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        let Some(target) = rule.get("outbound").and_then(Value::as_str) else {
            continue;
        };
        if business(target) && editable(rule) {
            if let Some(selection) = selections.get(target) {
                rule["outbound"] = json!(restore_group(
                    selection,
                    &original_outbounds,
                    &selections,
                    &mut names,
                    &mut restored
                ));
            }
        }
    }
    let final_choice = selections
        .get("final")
        .filter(|choice| choice.as_str() != "final")
        .map(|choice| {
            restore_group(
                choice,
                &original_outbounds,
                &selections,
                &mut names,
                &mut restored,
            )
        });
    migrated["outbounds"]
        .as_array_mut()
        .ok_or("outbound configuration unavailable")?
        .extend(restored);
    let mut policy = default_policy(&migrated);
    if let Some(choice) = final_choice {
        policy["final"] = json!(choice);
    }
    if original_outbounds
        .iter()
        .any(|outbound| tag(outbound) == "proxy" && outbound["default"] == "chain")
    {
        for rule in policy["rules"].as_array_mut().into_iter().flatten() {
            if rule["target"] == "proxy-auto" {
                rule["target"] = json!("proxy");
            }
        }
        if policy["final"] == "proxy-auto" {
            policy["final"] = json!("proxy");
        }
    }
    Ok(policy)
}

fn load(app: &App) -> Result<Snapshot, String> {
    let config_text =
        read_text(&app.moddir.join(CONFIG))?.ok_or("routing configuration unavailable")?;
    let config: Value =
        serde_json::from_str(&config_text).map_err(|_| "invalid routing configuration")?;
    let intent_text = read_text(&app.moddir.join(INTENT))?;
    let policy = match intent_text.as_deref() {
        Some(text) => serde_json::from_str(text).map_err(|_| "invalid saved routing policy")?,
        None => default_policy_for_app(app, &config)?,
    };
    validate_policy(&config, &policy, false)?;
    let revision = format!(
        "{:x}",
        Sha256::digest(format!(
            "{}\0{}",
            config_text,
            intent_text.as_deref().unwrap_or_default()
        ))
    );
    if read_text(&app.moddir.join(CONFIG))?.as_deref() != Some(&config_text)
        || read_text(&app.moddir.join(INTENT))? != intent_text
    {
        return Err("routing configuration changed; reload before editing".into());
    }
    Ok(Snapshot {
        config,
        config_text,
        intent_text,
        policy,
        revision,
    })
}

pub(crate) fn inspect_snapshot(app: &App) -> Result<(Value, Value), String> {
    let snapshot = load(app)?;
    let available: HashSet<String> = targets(&snapshot.config)
        .into_iter()
        .map(|value| value["tag"].as_str().unwrap().to_string())
        .collect();
    let mut policy = snapshot.policy;
    policy["revision"] = json!(snapshot.revision);
    policy["source"] = json!(if snapshot.intent_text.is_some() {
        "user_intent"
    } else {
        "defaults"
    });
    policy["protected_count"] = json!(array(&snapshot.config["route"], "rules")
        .iter()
        .filter(|rule| !editable(rule))
        .count());
    let mut choices = targets(&snapshot.config);
    for group in array(&policy, "groups") {
        if !choices.iter().any(|choice| tag(choice) == tag(&group)) {
            choices.push(json!({"tag": group["tag"], "type": group["type"]}));
        }
    }
    policy["targets"] = json!(choices);
    policy["rule_sets"] = json!(array(&snapshot.config["route"], "rule_set")
        .iter()
        .map(tag)
        .collect::<Vec<_>>());
    let groups: HashSet<String> = array(&policy, "groups")
        .iter()
        .map(|group| tag(group).to_string())
        .collect();
    for rule in policy["rules"]
        .as_array_mut()
        .ok_or("invalid saved rule list")?
    {
        rule["available"] = json!(rule["target"]
            .as_str()
            .is_some_and(|target| available.contains(target) || groups.contains(target)));
    }
    Ok((snapshot.config, policy))
}

fn targets(config: &Value) -> Vec<Value> {
    let mut targets: Vec<Value> = array(config, "outbounds")
        .iter()
        .filter(|outbound| {
            !business(tag(outbound))
                && !SYSTEM.contains(&tag(outbound))
                && !tag(outbound).starts_with("magicnet-")
        })
        .map(|outbound| json!({"tag": tag(outbound), "type": outbound["type"]}))
        .collect();
    for primary in ["proxy-auto", "final"] {
        if !targets.iter().any(|outbound| tag(outbound) == primary) {
            targets.push(json!({"tag": primary, "type": "selector"}));
        }
    }
    targets
}

fn valid_name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

fn validate_conditions(
    value: &Value,
    rule_sets: &HashSet<String>,
    depth: usize,
    check_references: bool,
) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or("matching conditions must be an object")?;
    if object.is_empty()
        || depth > 4
        || object.keys().any(|key| !MATCH_KEYS.contains(&key.as_str()))
    {
        return Err("unsupported or empty matching conditions".into());
    }
    if object.get("type").and_then(Value::as_str) == Some("logical") {
        if object
            .get("invert")
            .is_some_and(|value| !value.is_boolean())
        {
            return Err("matching flag must be boolean".into());
        }
        if !matches!(
            object.get("mode").and_then(Value::as_str),
            Some("and" | "or")
        ) || object
            .keys()
            .any(|key| !["type", "mode", "rules", "invert"].contains(&key.as_str()))
        {
            return Err("invalid logical rule".into());
        }
        let children = object
            .get("rules")
            .and_then(Value::as_array)
            .ok_or("logical rule requires children")?;
        if children.is_empty() || children.len() > 32 {
            return Err("invalid logical rule size".into());
        }
        for child in children {
            validate_conditions(child, rule_sets, depth + 1, check_references)?;
        }
    } else {
        if object
            .keys()
            .any(|key| ["type", "mode", "rules"].contains(&key.as_str()))
            || object.keys().all(|key| key == "invert")
        {
            return Err("a rule must contain matching conditions".into());
        }
        for (key, value) in object {
            if ["invert", "ip_is_private", "source_ip_is_private"].contains(&key.as_str()) {
                if !value.is_boolean() {
                    return Err("matching flag must be boolean".into());
                }
                continue;
            }
            let values = match value {
                Value::Array(values) => values.clone(),
                value => vec![value.clone()],
            };
            if values.is_empty() || values.len() > 256 {
                return Err("matching list exceeds its limit".into());
            }
            for item in values {
                match item {
                    Value::String(text) if valid_name(&text) => {
                        if check_references && key == "rule_set" && !rule_sets.contains(&text) {
                            return Err("unknown rule set".into());
                        }
                    }
                    Value::Number(number)
                        if number.as_u64().is_some()
                            && ["port", "source_port", "ip_version"].contains(&key.as_str()) => {}
                    _ => return Err("invalid matching value".into()),
                }
            }
        }
    }
    Ok(())
}

fn validate_policy(config: &Value, policy: &Value, strict: bool) -> Result<(), String> {
    let object = policy.as_object().ok_or("policy must be an object")?;
    if policy["schema"] != 1
        || object
            .keys()
            .any(|key| !["schema", "rules", "groups", "final"].contains(&key.as_str()))
    {
        return Err("unsupported routing policy schema".into());
    }
    let rules = policy["rules"].as_array().ok_or("policy requires rules")?;
    let groups = policy["groups"]
        .as_array()
        .ok_or("policy requires groups")?;
    if rules.len() > MAX_RULES || groups.len() > MAX_GROUPS {
        return Err("routing policy exceeds editing limits".into());
    }
    let base: HashSet<String> = targets(config)
        .into_iter()
        .filter(|outbound| !user_group(outbound))
        .map(|outbound| outbound["tag"].as_str().unwrap().to_string())
        .collect();
    let mut known = base.clone();
    for group in groups {
        let name = group["tag"].as_str().ok_or("group requires a name")?;
        if !valid_name(name)
            || SYSTEM.contains(&name)
            || name.starts_with("magicnet-")
            || !known.insert(name.to_string())
        {
            return Err("duplicate or reserved policy group name".into());
        }
    }
    let rule_sets: HashSet<String> = array(&config["route"], "rule_set")
        .iter()
        .map(|rule| tag(rule).to_string())
        .collect();
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for outbound in array(config, "outbounds").into_iter().filter(|outbound| {
        !business(tag(outbound)) && !user_group(outbound) && tag(outbound) != "final"
    }) {
        graph.insert(
            tag(&outbound).to_string(),
            array(&outbound, "outbounds")
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
        );
    }
    for group in groups {
        let object = group.as_object().ok_or("invalid policy group")?;
        if object
            .keys()
            .any(|key| !["tag", "type", "members", "default", "url"].contains(&key.as_str()))
        {
            return Err("unsupported group fields".into());
        }
        let members = group["members"]
            .as_array()
            .ok_or("group requires members")?;
        if members.is_empty() || members.len() > 256 {
            return Err("group requires bounded members".into());
        }
        let mut seen = HashSet::new();
        for member in members {
            let member = member
                .as_str()
                .filter(|member| valid_name(member))
                .ok_or("invalid group member")?;
            if !seen.insert(member) || (strict && !known.contains(member)) {
                return Err("duplicate or unavailable group member".into());
            }
        }
        match group["type"].as_str() {
            Some("selector")
                if members.contains(&group["default"]) && group.get("url").is_none() => {}
            Some("urltest") if group.get("default").is_none() => {
                let url = group["url"]
                    .as_str()
                    .ok_or("automatic group requires a probe URL")?;
                if url.len() > 2048
                    || !url.starts_with("https://")
                    || url.chars().any(char::is_control)
                    || url[8..]
                        .split('/')
                        .next()
                        .is_none_or(|host| host.is_empty() || host.contains('@'))
                {
                    return Err("probe requires an HTTPS URL without credentials".into());
                }
                if strict
                    && seen.iter().any(|member| {
                        !array(config, "outbounds").iter().any(|outbound| {
                            tag(outbound) == *member && outbound.get("server").is_some()
                        })
                    })
                {
                    return Err("automatic groups require individual proxy nodes".into());
                }
            }
            _ => return Err("invalid group type or default".into()),
        }
        graph.insert(
            tag(group).to_string(),
            seen.into_iter().map(str::to_string).collect(),
        );
    }
    let final_target = policy["final"]
        .as_str()
        .ok_or("policy requires a final target")?;
    if final_target == "final" || (strict && !known.contains(final_target)) {
        return Err("invalid final target".into());
    }
    graph.insert("final".into(), vec![final_target.into()]);
    fn visit(
        name: &str,
        graph: &HashMap<String, Vec<String>>,
        active: &mut HashSet<String>,
        done: &mut HashSet<String>,
    ) -> Result<(), String> {
        if done.contains(name) {
            return Ok(());
        }
        if !active.insert(name.to_string()) {
            return Err("policy group references form a cycle".into());
        }
        for member in graph.get(name).into_iter().flatten() {
            visit(member, graph, active, done)?;
        }
        active.remove(name);
        done.insert(name.to_string());
        Ok(())
    }
    let mut done = HashSet::new();
    for name in graph.keys() {
        visit(name, &graph, &mut HashSet::new(), &mut done)?;
    }
    for rule in rules {
        if rule.as_object().is_none_or(|object| {
            object
                .keys()
                .any(|key| !["conditions", "target"].contains(&key.as_str()))
        }) {
            return Err("unsupported rule fields".into());
        }
        validate_conditions(&rule["conditions"], &rule_sets, 0, strict)?;
        let target = rule["target"]
            .as_str()
            .filter(|target| valid_name(target))
            .ok_or("invalid rule target")?;
        let mut rendered = rule["conditions"].clone();
        rendered["outbound"] = json!(target);
        if !editable(&rendered) {
            return Err("rule conflicts with a protected system boundary".into());
        }
        if strict && !known.contains(target) {
            return Err("rule references an unavailable outbound".into());
        }
    }
    Ok(())
}

fn materialize(mut config: Value, policy: &Value) -> Result<Value, String> {
    validate_policy(&config, policy, false)?;
    let rule_sets = array(&config["route"], "rule_set")
        .iter()
        .map(|rule| tag(rule).to_string())
        .collect();
    for rule in array(policy, "rules") {
        validate_conditions(&rule["conditions"], &rule_sets, 0, true)?;
    }
    let mut outbounds: Vec<Value> = array(&config, "outbounds")
        .into_iter()
        .filter(|outbound| {
            !business(tag(outbound)) && !user_group(outbound) && tag(outbound) != "final"
        })
        .collect();
    let nodes: Vec<Value> = outbounds
        .iter()
        .filter(|outbound| {
            outbound.get("server").is_some() && !tag(outbound).starts_with("magicnet-")
        })
        .map(|outbound| json!(tag(outbound)))
        .collect();
    for outbound in &mut outbounds {
        if tag(outbound) == "proxy" {
            let mut members = nodes.clone();
            if !nodes.is_empty() {
                members.insert(0, json!("proxy-auto"));
            }
            if members.is_empty() {
                members.push(json!("block"));
            }
            for option in ["chain", "direct", "block"] {
                if option == "chain"
                    && !array(&config, "outbounds")
                        .iter()
                        .any(|outbound| tag(outbound) == "chain")
                {
                    continue;
                }
                if !members.contains(&json!(option)) {
                    members.push(json!(option));
                }
            }
            outbound["outbounds"] = json!(members);
            let chain_selected = array(&config, "outbounds")
                .iter()
                .any(|outbound| tag(outbound) == "proxy" && outbound["default"] == "chain");
            outbound["default"] = json!(if chain_selected {
                "chain"
            } else if nodes.is_empty() {
                "block"
            } else {
                "proxy-auto"
            });
        }
    }
    if nodes.is_empty() {
        outbounds.retain(|outbound| tag(outbound) != "proxy-auto");
        outbounds.push(json!({"type": "selector", "tag": "proxy-auto", "outbounds": ["block"], "default": "block"}));
    } else if !outbounds
        .iter()
        .any(|outbound| tag(outbound) == "proxy-auto")
    {
        outbounds.push(json!({"type": "urltest", "tag": "proxy-auto", "outbounds": nodes, "url": "https://www.gstatic.com/generate_204", "interval": "3m", "tolerance": 30}));
    }
    let mut known: HashSet<String> = outbounds
        .iter()
        .map(|outbound| tag(outbound).to_string())
        .collect();
    for group in array(policy, "groups") {
        known.insert(tag(&group).to_string());
    }
    for group in array(policy, "groups") {
        let mut members: Vec<Value> = array(&group, "members")
            .into_iter()
            .filter(|member| member.as_str().is_some_and(|name| known.contains(name)))
            .collect();
        let mut outbound = json!({"tag": group["tag"], "type": group["type"]});
        if members.is_empty() {
            outbound["type"] = json!("selector");
            outbound["default"] = json!("block");
            members.push(json!("block"));
        } else if group["type"] == "urltest" {
            outbound["url"] = group["url"].clone();
            outbound["interval"] = json!("3m");
            outbound["tolerance"] = json!(30);
        } else if members.contains(&group["default"]) {
            outbound["default"] = group["default"].clone();
        } else {
            if !members.contains(&json!("block")) {
                members.push(json!("block"));
            }
            outbound["default"] = json!("block");
        }
        outbound["outbounds"] = json!(members);
        outbounds.push(outbound);
    }
    let effective = |target: &Value| {
        if target.as_str().is_some_and(|name| known.contains(name)) {
            target.clone()
        } else {
            json!("block")
        }
    };
    outbounds.push(json!({"type": "selector", "tag": "final", "outbounds": [effective(&policy["final"])], "default": effective(&policy["final"])}));
    config["outbounds"] = json!(outbounds);
    let old_rules = array(&config["route"], "rules");
    let mut rules: Vec<Value> = old_rules
        .iter()
        .filter(|rule| !editable(rule))
        .cloned()
        .collect();
    for rule in &mut rules {
        remap(rule);
    }
    for rule in array(policy, "rules") {
        let mut rendered = rule["conditions"].clone();
        rendered["outbound"] = effective(&rule["target"]);
        rules.push(rendered);
    }
    config["route"]["rules"] = json!(rules);
    config["route"]["final"] = json!("final");
    Ok(config)
}

fn materialize_for_app(app: &App, config: Value, policy: &Value) -> Result<Value, String> {
    let old_config = config.clone();
    let mut candidate = materialize(config, policy)?;
    if let Some(selected) = saved_selections(app)?.get("proxy") {
        for outbound in candidate["outbounds"].as_array_mut().into_iter().flatten() {
            if tag(outbound) == "proxy" {
                outbound["default"] = if array(outbound, "outbounds").contains(&json!(selected)) {
                    json!(selected)
                } else {
                    json!("block")
                };
            }
        }
    }
    if read_text(&app.moddir.join(".config/magicnet/dns-rules.json"))?.is_none() {
        let profile = crate::dns::dns_profile(app);
        let application = if profile == "default" {
            "default-remote-dns".to_string()
        } else {
            format!(
                "{}-profile-dns",
                profile.split('-').next().unwrap_or("cloudflare")
            )
        };
        project_dns(&old_config, &mut candidate, policy, &application);
    }
    Ok(candidate)
}

fn project_dns(old: &Value, candidate: &mut Value, policy: &Value, application: &str) {
    if !old["dns"].is_object() {
        return;
    }
    let servers: HashSet<String> = array(&old["dns"], "servers")
        .iter()
        .map(|server| tag(server).to_string())
        .collect();
    let remote = [
        application,
        old["dns"]["final"].as_str().unwrap_or_default(),
        "doh-google",
        "doh-cloudflare",
    ]
    .into_iter()
    .find(|server| *server != "bootstrap-local-dns" && servers.contains(*server));
    let Some(remote) = remote else {
        return;
    };
    if !servers.contains("bootstrap-local-dns") {
        return;
    }
    let dns_keys = [
        "domain",
        "domain_suffix",
        "domain_keyword",
        "domain_regex",
        "rule_set",
        "invert",
        "type",
        "mode",
        "rules",
    ];
    fn dns_conditions(value: &Value, keys: &[&str]) -> bool {
        value.as_object().is_some_and(|object| {
            object.keys().all(|key| keys.contains(&key.as_str()))
                && object.get("rules").is_none_or(|children| {
                    children.as_array().is_some_and(|children| {
                        children.iter().all(|child| dns_conditions(child, keys))
                    })
                })
        })
    }
    let protected: Vec<Value> = array(&old["route"], "rules")
        .iter()
        .filter(|rule| !editable(rule))
        .filter_map(|rule| {
            let mut object = rule.as_object()?.clone();
            object.remove("outbound");
            object.remove("action");
            Some(Value::Object(object))
        })
        .collect();
    let mut rules: Vec<Value> = array(&old["dns"], "rules")
        .into_iter()
        .filter(|rule| {
            let mut conditions = rule.as_object().cloned().unwrap_or_default();
            let server = conditions.remove("server").unwrap_or(Value::Null);
            let action = conditions.remove("action");
            action
                .as_ref()
                .is_some_and(|action| action.as_str() != Some("reject"))
                || !dns_conditions(&Value::Object(conditions.clone()), &dns_keys)
                || protected.contains(&Value::Object(conditions))
                || server.as_str().is_some_and(|server| {
                    server.starts_with("lan-") || server.contains("tailscale")
                })
        })
        .collect();
    for rule in array(policy, "rules") {
        if !dns_conditions(&rule["conditions"], &dns_keys) {
            continue;
        }
        let mut projected = rule["conditions"].clone();
        if rule["target"] == "block" {
            projected["action"] = json!("reject");
        } else {
            projected["server"] = if rule["target"] == "direct" {
                json!("bootstrap-local-dns")
            } else {
                json!(remote)
            };
        }
        if !rules.contains(&projected) {
            rules.push(projected);
        }
    }
    candidate["dns"]["rules"] = json!(rules);
}

pub(crate) fn command(app: &App, args: &[String]) -> Result<(), String> {
    match args {
        [action, path] if action == "save-file" => save(app, Path::new(path)),
        [action, remaining @ ..] if action == "domain" => legacy_domain(app, remaining),
        [action, path] if action == "render" => {
            let path = Path::new(path);
            let relative = path
                .strip_prefix(&app.moddir)
                .map_err(|_| "candidate is outside the module")?;
            if relative
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
                || !relative.starts_with(".config/sing-box")
            {
                return Err("invalid candidate path".into());
            }
            let text = read_text(path)?.ok_or("candidate unavailable")?;
            let config: Value = serde_json::from_str(&text).map_err(|_| "invalid candidate")?;
            let intent = read_text(&app.moddir.join(INTENT))?;
            if intent.is_none()
                && app
                    .moddir
                    .join(".config/sing-box/standalone-config")
                    .exists()
            {
                return Ok(());
            }
            let migrating = path == app.moddir.join(CONFIG)
                && intent.is_none()
                && saved_selections(app)?
                    .iter()
                    .any(|(name, selected)| business(name) || business(selected));
            let policy = match intent {
                Some(text) => serde_json::from_str(&text).map_err(|_| "invalid saved policy")?,
                None => default_policy_for_app(app, &config)?,
            };
            let candidate = materialize_for_app(app, config, &policy)?;
            let rendered = encode(&candidate)?;
            if migrating {
                let intent = encode(&policy)?;
                replace_module_text_files_transactionally(
                    app,
                    &[(Path::new(INTENT), &intent), (relative, &rendered)],
                )?;
            } else if rendered != text {
                replace_module_text_files_transactionally(app, &[(relative, &rendered)])?;
            }
            Ok(())
        }
        _ => Err(
            "Usage: cli routing {save-file <webui-payload-path>|render <module-candidate-path>}"
                .into(),
        ),
    }
}

fn save(app: &App, path: &Path) -> Result<(), String> {
    if !crate::dns_rules::inherited_config_lock(app) {
        return service::run_routing_policy_save(app, path);
    }
    let text = config_editor::read_webui_payload_text(app, path)?;
    let payload: Value = serde_json::from_str(&text).map_err(|_| "invalid routing payload")?;
    commit(app, payload)
}

fn commit(app: &App, mut payload: Value) -> Result<(), String> {
    let snapshot = load(app)?;
    if payload["revision"].as_str() != Some(&snapshot.revision) {
        return Err("routing configuration changed; reload before applying".into());
    }
    payload
        .as_object_mut()
        .ok_or("invalid routing payload")?
        .remove("revision");
    validate_policy(&snapshot.config, &payload, true)?;
    let candidate = materialize_for_app(app, snapshot.config.clone(), &payload)?;
    let candidate_text = encode(&candidate)?;
    let intent = encode(&payload)?;
    service::apply_routing_transaction_locked(
        app,
        || {
            config_editor::validate_singbox_config_text(app, &candidate_text)?;
            if load(app)?.revision != snapshot.revision {
                return Err("routing configuration changed during validation".into());
            }
            replace_module_text_files_transactionally(app, &[(Path::new(INTENT), &intent)])
        },
        || {
            if let Some(previous) = snapshot.intent_text.as_deref() {
                replace_module_text_files_transactionally(
                    app,
                    &[
                        (Path::new(INTENT), previous),
                        (Path::new(CONFIG), &snapshot.config_text),
                    ],
                )
            } else {
                replace_module_text_files_transactionally(
                    app,
                    &[(Path::new(CONFIG), &snapshot.config_text)],
                )?;
                match fs::remove_file(app.moddir.join(INTENT)) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(_) => Err("unable to restore routing intent".into()),
                }
            }
        },
    )?;
    println!("[info] Routing policy validated, saved and applied");
    Ok(())
}

pub(crate) fn has_intent(app: &App) -> bool {
    app.moddir.join(INTENT).exists()
}

pub(crate) fn list(app: &App) -> Result<(), String> {
    let snapshot = load(app)?;
    for rule in array(&snapshot.policy, "rules") {
        println!(
            "{} -> {}",
            rule["conditions"],
            rule["target"].as_str().unwrap_or("unknown")
        );
    }
    Ok(())
}

pub(crate) fn legacy_domain(app: &App, args: &[String]) -> Result<(), String> {
    if !crate::dns_rules::inherited_config_lock(app) {
        let mut command = vec!["domain".to_string()];
        command.extend_from_slice(args);
        return service::run_routing_policy_command(app, &command);
    }
    let snapshot = load(app)?;
    let mut policy = snapshot.policy;
    let valid_domain = |domain: &str| {
        !domain.is_empty()
            && domain.len() <= 253
            && !domain
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
    };
    let mut remove = |target: &str, domain: &str| -> Result<(), String> {
        let rules = policy["rules"]
            .as_array_mut()
            .ok_or("invalid saved rules")?;
        let mut found = false;
        let target = mapped_target(target);
        for rule in rules.iter_mut().filter(|rule| {
            rule["target"] == target
                && rule["conditions"]
                    .as_object()
                    .is_some_and(|object| object.len() == 1)
        }) {
            if let Some(values) = rule["conditions"]["domain_suffix"].as_array_mut() {
                let previous = values.len();
                values.retain(|value| value.as_str() != Some(domain));
                found |= previous != values.len();
            }
        }
        rules.retain(|rule| {
            rule["conditions"]["domain_suffix"]
                .as_array()
                .is_none_or(|values| !values.is_empty())
        });
        if !found {
            return Err("domain suffix not found in the selected route".into());
        }
        Ok(())
    };
    let addition = match args {
        [action, target, domain] if action == "add-domain" && valid_domain(domain) => {
            Some((target, domain))
        }
        [action, target, domain] if action == "remove-domain" && valid_domain(domain) => {
            remove(target, domain)?;
            None
        }
        [action, old_target, new_target, old_domain, new_domain]
            if action == "edit-domain" && valid_domain(old_domain) && valid_domain(new_domain) =>
        {
            remove(old_target, old_domain)?;
            Some((new_target, new_domain))
        }
        _ => return Err("invalid legacy domain command".into()),
    };
    if let Some((target, domain)) = addition {
        let target = mapped_target(target);
        policy["rules"]
            .as_array_mut()
            .ok_or("invalid saved rules")?
            .insert(
                0,
                json!({"conditions":{"domain_suffix":[domain]},"target":target}),
            );
    }
    policy["revision"] = json!(snapshot.revision);
    commit(app, policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Value {
        json!({"outbounds": [
            {"tag":"node-a","type":"socks","server":"127.0.0.1","server_port":1080},
            {"tag":"proxy-auto","type":"urltest","outbounds":["node-a"],"url":"https://example.com/"},
            {"tag":"proxy","type":"selector","outbounds":["node-a","proxy-auto"],"default":"node-a"},
            {"tag":"direct","type":"direct"},{"tag":"block","type":"block"},
            {"tag":"ai-grok","type":"selector","outbounds":["node-a"]},
            {"tag":"github-proxy","type":"selector","outbounds":["proxy"]}
        ], "route":{"rule_set":[{"tag":"grok"},{"tag":"github"}],"rules":[
            {"action":"hijack-dns","protocol":"dns"},
            {"rule_set":["grok"],"outbound":"ai-grok"},
            {"rule_set":["github"],"outbound":"github-proxy"}
        ]}})
    }

    #[test]
    fn defaults_merge_adjacent_classifiers_and_remove_business_groups() {
        let config = config();
        let policy = default_policy(&config);
        assert_eq!(
            policy["rules"],
            json!([{ "conditions":{"rule_set":["grok","github"]},"target":"proxy-auto" }])
        );
        let rendered = materialize(config, &policy).unwrap();
        assert_eq!(rendered["route"]["rules"][0]["action"], "hijack-dns");
        assert!(!array(&rendered, "outbounds")
            .iter()
            .any(|outbound| business(tag(outbound))));
        assert_eq!(rendered["route"]["final"], "final");
        assert_eq!(materialize(rendered.clone(), &policy).unwrap(), rendered);
    }

    #[test]
    fn user_rules_and_groups_survive_subscription_changes_and_fail_closed() {
        let mut policy = default_policy(&config());
        policy["groups"] =
            json!([{"tag":"my-ai","type":"selector","members":["node-a"],"default":"node-a"}]);
        policy["rules"] = json!([{"conditions":{"domain_suffix":["grok.com"]},"target":"my-ai"}]);
        validate_policy(&config(), &policy, true).unwrap();
        let mut next = config();
        next["outbounds"]
            .as_array_mut()
            .unwrap()
            .retain(|outbound| tag(outbound) != "node-a");
        let rendered = materialize(next, &policy).unwrap();
        let group = array(&rendered, "outbounds")
            .into_iter()
            .find(|outbound| tag(outbound) == "my-ai")
            .unwrap();
        assert_eq!(group["default"], "block");
        assert_eq!(
            rendered["route"]["rules"][1]["domain_suffix"],
            json!(["grok.com"])
        );
    }

    #[test]
    fn rejects_cycles_deleted_references_and_protected_conditions() {
        let mut policy = default_policy(&config());
        policy["groups"] =
            json!([{"tag":"custom","type":"selector","members":["final"],"default":"final"}]);
        policy["final"] = json!("custom");
        assert!(validate_policy(&config(), &policy, true)
            .unwrap_err()
            .contains("cycle"));
        policy = default_policy(&config());
        policy["rules"][0]["target"] = json!("removed-group");
        assert!(validate_policy(&config(), &policy, true).is_err());
        policy["rules"][0] = json!({"conditions":{"user_id":[10001]},"target":"proxy-auto"});
        assert!(validate_policy(&config(), &policy, true).is_err());
    }

    #[test]
    fn does_not_merge_across_a_direct_exception_or_different_conditions() {
        let mut config = config();
        config["route"]["rules"].as_array_mut().unwrap().insert(
            2,
            json!({"domain_suffix":["exception.invalid"],"outbound":"direct"}),
        );
        assert_eq!(
            default_policy(&config)["rules"].as_array().unwrap().len(),
            3
        );
    }

    #[test]
    fn explicit_legacy_service_choices_migrate_to_direct_rule_targets() {
        let app = crate::test_support::temp_app();
        fs::create_dir_all(app.moddir.join(".config/magicnet")).unwrap();
        fs::write(
            app.moddir.join(".config/magicnet/selector-selections.json"),
            r#"{"ai-grok":"node-a"}"#,
        )
        .unwrap();
        let mut config = config();
        config["route"]["rules"][1]["outbound"] = json!("ai-grok");
        let policy = default_policy_for_app(&app, &config).unwrap();
        assert_eq!(policy["rules"][0]["target"], "node-a");
        assert!(policy["groups"].as_array().unwrap().is_empty());
    }

    #[test]
    fn module_app_rules_and_empty_legacy_placeholders_remain_outside_user_intent() {
        let mut config = config();
        config["route"]["rules"].as_array_mut().unwrap().insert(
            1,
            json!({"package_name":["__magicnet_app_proxy__","com.example.app"],"outbound":"proxy"}),
        );
        config["route"]["rules"]
            .as_array_mut()
            .unwrap()
            .push(json!({"domain_suffix":["__magicnet_route__"],"outbound":"proxy"}));
        let policy = default_policy(&config);
        assert_eq!(policy["rules"].as_array().unwrap().len(), 1);
        validate_policy(&config, &policy, true).unwrap();
        let rendered = materialize(config, &policy).unwrap();
        assert_eq!(
            rendered["route"]["rules"][1]["package_name"][0],
            "__magicnet_app_proxy__"
        );
    }

    #[test]
    fn missing_rule_sets_allow_inspection_and_repair_but_not_unsafe_rendering() {
        let original = config();
        let policy = default_policy(&original);
        let mut changed = original;
        changed["route"]["rule_set"] = json!([]);
        validate_policy(&changed, &policy, false).unwrap();
        assert!(validate_policy(&changed, &policy, true).is_err());
        assert!(materialize(changed, &policy).is_err());
    }

    #[test]
    fn nested_legacy_choices_and_final_selection_are_preserved() {
        let app = crate::test_support::temp_app();
        fs::create_dir_all(app.moddir.join(".config/magicnet")).unwrap();
        fs::write(
            app.moddir.join(".config/magicnet/selector-selections.json"),
            r#"{"ai-grok":"github-proxy","github-proxy":"node-a","final":"ai-grok"}"#,
        )
        .unwrap();
        let policy = default_policy_for_app(&app, &config()).unwrap();
        assert_eq!(policy["final"], "user-ai-grok");
        let groups = array(&policy, "groups");
        assert_eq!(
            groups
                .iter()
                .find(|group| tag(group) == "user-github-proxy")
                .unwrap()["default"],
            "node-a"
        );
        assert_eq!(
            groups
                .iter()
                .find(|group| tag(group) == "user-ai-grok")
                .unwrap()["default"],
            "user-github-proxy"
        );
        validate_policy(&config(), &policy, true).unwrap();
    }

    #[test]
    fn published_configuration_stays_inside_the_read_size_limit() {
        assert!(encode(&json!({"value":"x".repeat(MAX_BYTES as usize)})).is_err());
        assert!(encode(&config()).is_ok());
    }

    #[test]
    fn dns_projection_preserves_profile_and_protected_rules_without_duplicate_rejects() {
        let app = crate::test_support::temp_app();
        let mut original = config();
        original["dns"] = json!({"final":"bootstrap-local-dns","servers":[
            {"tag":"bootstrap-local-dns"},{"tag":"doh-google"},{"tag":"doh-cloudflare"}
        ],"rules":[
            {"rule_set":["protected"],"server":"doh-cloudflare"},
            {"rule_set":["grok"],"server":"doh-google"}
        ]});
        original["outbounds"].as_array_mut().unwrap().push(
            json!({"tag":"ad-block","type":"selector","outbounds":["block"],"default":"block"}),
        );
        original["route"]["rules"]
            .as_array_mut()
            .unwrap()
            .insert(1, json!({"rule_set":["protected"],"outbound":"ad-block"}));
        let mut policy = default_policy(&original);
        policy["rules"]
            .as_array_mut()
            .unwrap()
            .push(json!({"conditions":{"domain_suffix":["blocked.invalid"]},"target":"block"}));
        let first = materialize_for_app(&app, original, &policy).unwrap();
        let second = materialize_for_app(&app, first.clone(), &policy).unwrap();
        assert_eq!(first, second);
        assert_eq!(first["dns"]["rules"][0]["server"], "doh-cloudflare");
        assert_eq!(first["dns"]["rules"][1]["server"], "doh-google");
        assert_eq!(
            array(&first["dns"], "rules")
                .iter()
                .filter(|rule| rule["action"] == "reject")
                .count(),
            1
        );
    }
}
