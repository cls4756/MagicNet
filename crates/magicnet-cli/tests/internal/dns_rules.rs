use super::*;
use std::os::unix::fs::symlink;
use std::sync::atomic::{AtomicUsize, Ordering};

static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct Fixture(App);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "magicnet-dns-rules-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join(".config/sing-box")).unwrap();
        fs::create_dir_all(root.join(".config/magicnet")).unwrap();
        let fixture = Self(App::for_test(root));
        fs::write(
            fixture.0.moddir.join(".config/magicnet/dns.conf"),
            "MAGICNET_DNS_PROFILE=cloudflare-doh\n",
        )
        .unwrap();
        let config = json!({
            "dns": {
                "servers": [
                    {"tag": BOOTSTRAP_SERVER, "type": "udp", "server": "192.168.99.1"},
                    {"tag": "cloudflare-profile-dns", "type": "https", "server": "private-resolver.invalid", "password": "resolver-private-password"},
                    {"tag": "tailscale-dns", "type": "tailscale"},
                    {"tag": "custom-dns", "type": "udp", "server": "192.168.99.2"}
                ],
                "rules": [
                    {"clash_mode": "Global", "server": "cloudflare-profile-dns"},
                    {"domain_suffix": ["cn"], "server": BOOTSTRAP_SERVER},
                    {"rule_set": ["foreign"], "server": "cloudflare-profile-dns"},
                    {"domain": ["feature.invalid"], "server": "tailscale-dns"},
                    {"type": "logical", "mode": "and", "rules": [{"domain": "advanced.invalid"}], "server": "custom-dns", "private_field": "private-advanced-data"}
                ],
                "final": "cloudflare-profile-dns"
            },
            "route": {"default_domain_resolver": {"server": BOOTSTRAP_SERVER}, "rule_set": [{"tag": "foreign", "url": "https://private-rule-set.invalid"}]},
            "outbounds": [{"tag": "private-node-name", "password": "private-node-password"}]
        });
        fs::write(fixture.0.moddir.join(CONFIG_PATH), config.to_string()).unwrap();
        fixture
    }

    fn snapshot(&self) -> Snapshot {
        load_snapshot(&self.0).unwrap()
    }

    fn payload(&self) -> Value {
        let snapshot = self.snapshot();
        json!({"schema": 1, "revision": snapshot.revision, "rules": (1..=snapshot.rules.len()).map(|id| json!({"id": id})).collect::<Vec<_>>()})
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0.moddir);
    }
}

#[test]
fn lan_dns_rules_are_owned_protected_and_not_persisted_as_user_overrides() {
    let fixture = Fixture::new();
    let mut config = fixture.snapshot().config;
    config["dns"]["servers"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "tag": LAN_SERVER, "type": "udp", "server": "192.168.88.1"
        }));
    let lan_rule = json!({"domain_suffix": ["router.lan"], "server": LAN_SERVER});
    config["dns"]["rules"]
        .as_array_mut()
        .unwrap()
        .insert(0, lan_rule.clone());
    fs::write(fixture.0.moddir.join(CONFIG_PATH), config.to_string()).unwrap();
    let observation = inspect(&fixture.0).unwrap();
    assert_eq!(observation["rules"][0]["protected_reason"], "feature");
    assert_eq!(observation["rules"][0]["editable"], false);
    assert!(observation["resolvers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|resolver| resolver["tag"] != LAN_SERVER));
    assert!(!observation.to_string().contains("192.168.88.1"));
    let snapshot = fixture.snapshot();
    let payload = fixture.payload();
    let rules = validate_payload(&snapshot, &payload).unwrap();
    assert!(rules.iter().all(|rule| rule["server"] != LAN_SERVER));
    assert_eq!(
        materialize(&snapshot, &rules).unwrap()["dns"]["rules"][0],
        lan_rule
    );
    let mut changed = payload.clone();
    changed["rules"][0]["target"] = json!(BOOTSTRAP_SERVER);
    assert!(validate_payload(&snapshot, &changed)
        .unwrap_err()
        .contains("protected"));
    let mut removed = payload;
    removed["rules"].as_array_mut().unwrap().remove(0);
    assert!(validate_payload(&snapshot, &removed).is_err());
}

#[test]
fn inspection_is_private_bounded_read_only_configuration_not_runtime_evidence() {
    let fixture = Fixture::new();
    let snapshot = inspect(&fixture.0).unwrap();
    assert_eq!(snapshot["runtime_state"], "unknown");
    assert_eq!(snapshot["source"], "configuration");
    assert_eq!(snapshot["node_resolver"], BOOTSTRAP_SERVER);
    assert_eq!(snapshot["rules"][0]["protected_reason"], "mode");
    assert_eq!(snapshot["rules"][1]["editable"], true);
    assert_eq!(snapshot["rules"][2]["target"], PROFILE_SERVER);
    assert_eq!(snapshot["rules"][3]["protected_reason"], "feature");
    assert_eq!(snapshot["rules"][4]["protected_reason"], "unsupported");
    assert_eq!(
        snapshot["rules"][4]["conditions"]["rules"][0]["domain"],
        "advanced.invalid"
    );
    let output = snapshot.to_string();
    for secret in [
        "192.168.99",
        "private-resolver",
        "resolver-private-password",
        "private-advanced-data",
        "private-rule-set",
        "private-node",
    ] {
        assert!(
            !output.contains(secret),
            "unexpected private field: {secret}"
        );
    }
    assert!(!fixture.0.moddir.join(".state").exists());
}

#[test]
fn protected_rules_cannot_be_modified_removed_or_moved_and_keep_raw_fields() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let payload = fixture.payload();
    let rules = validate_payload(&snapshot, &payload).unwrap();
    assert_eq!(
        rules.last().unwrap()["private_field"],
        "private-advanced-data"
    );
    assert!(!rules
        .iter()
        .any(|rule| feature_rule(rule, &snapshot.feature_servers)));
    for id in [0, 3, 4] {
        let mut modified = payload.clone();
        modified["rules"][id]["target"] = json!(BOOTSTRAP_SERVER);
        assert!(validate_payload(&snapshot, &modified).is_err());
        let mut removed = payload.clone();
        removed["rules"].as_array_mut().unwrap().remove(id);
        assert!(validate_payload(&snapshot, &removed).is_err());
        let mut moved = payload.clone();
        moved["rules"].as_array_mut().unwrap().swap(id, 1);
        assert!(validate_payload(&snapshot, &moved).is_err());
    }
}

#[test]
fn ordinary_rules_can_be_edited_reordered_and_appended_without_retargeting_bootstrap() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let mut payload = fixture.payload();
    payload["rules"][2] = json!({"id": 3, "conditions": {"domain_suffix": ["remote.invalid"]}, "target": PROFILE_SERVER});
    payload["rules"].as_array_mut().unwrap().swap(1, 2);
    payload["rules"].as_array_mut().unwrap().push(
        json!({"id": null, "conditions": {"domain": "custom.invalid"}, "target": "custom-dns"}),
    );
    let rules = validate_payload(&snapshot, &payload).unwrap();
    let materialized = materialize(&snapshot, &rules).unwrap();
    assert_eq!(materialized["dns"]["rules"][0]["server"], "tailscale-dns");
    assert_eq!(
        materialized["dns"]["rules"][2]["server"],
        "cloudflare-profile-dns"
    );
    assert_eq!(materialized["dns"]["rules"][3]["server"], BOOTSTRAP_SERVER);
    assert_eq!(
        materialized["dns"]["rules"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["server"],
        "custom-dns"
    );
}

#[test]
fn validation_rejects_stale_revisions_duplicate_ids_unavailable_refs_and_invalid_shapes() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let payload = fixture.payload();
    for change in [
        json!({"id": 2, "conditions": {"rule_set": ["missing"]}, "target": PROFILE_SERVER}),
        json!({"id": 2, "conditions": {"domain": "valid.invalid"}, "target": "missing"}),
        json!({"id": 2, "conditions": {"domain": "valid.invalid"}, "target": "tailscale-dns"}),
        json!({"id": 2, "conditions": {"server": "custom-dns"}, "target": PROFILE_SERVER}),
        json!({"id": 2, "conditions": {}, "target": PROFILE_SERVER}),
        json!({"id": 2, "conditions": {"port": [0]}, "target": PROFILE_SERVER}),
        json!({"id": 2, "conditions": {"domain": ["newline\n.invalid"]}, "target": PROFILE_SERVER}),
        json!({"id": 2, "conditions": {"invert": "false"}, "target": PROFILE_SERVER}),
        json!({"id": 99}),
        json!({"id": 3}),
    ] {
        let mut candidate = payload.clone();
        candidate["rules"][1] = change;
        assert!(validate_payload(&snapshot, &candidate).is_err());
    }
    let mut stale = payload;
    stale["revision"] = json!("stale");
    assert!(validate_payload(&snapshot, &stale).is_err());
    let oversized = json!({"schema": 1, "revision": snapshot.revision, "rules": vec![json!({}); MAX_RULES + 1]});
    assert!(validate_payload(&snapshot, &oversized).is_err());
}

#[test]
fn saved_profile_alias_survives_default_round_trip_and_revision_tracks_preferences() {
    let fixture = Fixture::new();
    let original = fixture.snapshot();
    let rules = validate_payload(&original, &fixture.payload()).unwrap();
    fs::write(
        fixture.0.moddir.join(RULES_PATH),
        json!({"schema": 1, "rules": rules}).to_string(),
    )
    .unwrap();
    fs::write(
        fixture.0.moddir.join(".config/magicnet/dns.conf"),
        "MAGICNET_DNS_PROFILE=default\n",
    )
    .unwrap();
    let default = fixture.snapshot();
    assert_ne!(original.revision, default.revision);
    assert_eq!(default.application_server, BOOTSTRAP_SERVER);
    assert!(default
        .rules
        .iter()
        .any(|rule| rule["server"] == PROFILE_SERVER));
    let default_materialized = materialize(&default, &rules).unwrap();
    assert!(default_materialized["dns"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .all(|rule| rule["server"] != "cloudflare-profile-dns"));
    fs::write(
        fixture.0.moddir.join(".config/magicnet/dns.conf"),
        "MAGICNET_DNS_PROFILE=cloudflare-doh\n",
    )
    .unwrap();
    let restored = fixture.snapshot();
    assert_eq!(restored.application_server, "cloudflare-profile-dns");
    assert_eq!(inspect(&fixture.0).unwrap()["source"], "user_intent");
}

#[test]
fn explicit_default_route_metadata_remains_editable_and_is_preserved() {
    let fixture = Fixture::new();
    let mut config = fixture.snapshot().config;
    config["dns"]["rules"][1]["type"] = json!("default");
    config["dns"]["rules"][1]["action"] = json!("route");
    fs::write(fixture.0.moddir.join(CONFIG_PATH), config.to_string()).unwrap();
    let inspection = inspect(&fixture.0).unwrap();
    assert_eq!(inspection["rules"][1]["editable"], true);
    assert!(inspection["rules"][1]["conditions"].get("action").is_none());
    let mut payload = fixture.payload();
    payload["rules"][1] =
        json!({"id":2,"conditions":{"domain_suffix":["edited.invalid"]},"target":PROFILE_SERVER});
    let rules = validate_payload(&fixture.snapshot(), &payload).unwrap();
    assert_eq!(rules[1]["type"], "default");
    assert_eq!(rules[1]["action"], "route");
}

#[test]
fn malformed_symlink_and_oversized_intent_are_rejected_without_changing_config() {
    let fixture = Fixture::new();
    let config = fs::read(fixture.0.moddir.join(CONFIG_PATH)).unwrap();
    let path = fixture.0.moddir.join(RULES_PATH);
    for text in [
        "not json",
        "{\"schema\":2,\"rules\":[]}",
        "{\"schema\":1,\"rules\":[null]}",
    ] {
        fs::write(&path, text).unwrap();
        assert!(load_snapshot(&fixture.0).is_err());
    }
    fs::remove_file(&path).unwrap();
    symlink(fixture.0.moddir.join(CONFIG_PATH), &path).unwrap();
    assert!(load_snapshot(&fixture.0).is_err());
    fs::remove_file(&path).unwrap();
    let file = fs::File::create(&path).unwrap();
    file.set_len(MAX_BYTES + 1).unwrap();
    assert!(load_snapshot(&fixture.0).is_err());
    assert_eq!(
        fs::read(fixture.0.moddir.join(CONFIG_PATH)).unwrap(),
        config
    );
}
