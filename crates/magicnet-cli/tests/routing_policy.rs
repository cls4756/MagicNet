#![cfg(target_os = "linux")]

use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "magicnet-routing-'quoted-{}-{stamp}",
            std::process::id()
        ));
        for directory in [
            "bin",
            "lib/kamfw",
            "lib/magicnet/singbox_subscribe",
            ".tmp/webui-payload",
            ".config/magicnet",
            ".config/sing-box/rules",
        ] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        fs::write(root.join("module.prop"), "id=MagicNet\n").unwrap();
        fs::write(
            root.join("lib/kamfw/.kamfwrc"),
            "import() { :; }; info() { :; }; warn() { printf '%s\\n' \"$*\" >&2; }\n",
        )
        .unwrap();
        for (name, text) in [
            (
                "common.sh",
                include_str!("../../../src/MagicNet/lib/magicnet/common.sh"),
            ),
            (
                "primitives.sh",
                include_str!("../../../src/MagicNet/lib/magicnet/primitives.sh"),
            ),
            (
                "subscribe_bootstrap.sh",
                include_str!("../../../src/MagicNet/lib/magicnet/subscribe_bootstrap.sh"),
            ),
            (
                "singbox_subscribe/bootstrap.sh",
                include_str!("../../../src/MagicNet/lib/magicnet/singbox_subscribe/bootstrap.sh"),
            ),
        ] {
            fs::write(root.join("lib/magicnet").join(name), text).unwrap();
        }
        fs::write(root.join("lib/magicnet_singbox_subscribe.sh"), "magicnet_singbox_update_lock_active() { [ -d \"$MODDIR/.state/sing-box/subscription-update.lock\" ]; }\n").unwrap();
        fs::write(
            root.join("lib/magicnet.sh"),
            r#". "$MODDIR/lib/magicnet/common.sh"
magicnet_apply_runtime_config() {
    if [ -e "$MODDIR/fail-apply" ]; then
        rm "$MODDIR/fail-apply"
        printf '%s\n' 'partial-materialization' >"$MODDIR/.config/sing-box/config.json"
        return 1
    fi
    "$MODDIR/bin/magicnet-cli" routing render "$MODDIR/.config/sing-box/config.json"
}
"#,
        )
        .unwrap();
        fs::copy(
            env!("CARGO_BIN_EXE_magicnet-cli"),
            root.join("bin/magicnet-cli"),
        )
        .unwrap();
        for (name, script) in [
            ("pidof", "#!/bin/sh\nexit 1\n"),
            ("ps", "#!/bin/sh\nexit 1\n"),
            ("curl", "#!/bin/sh\nexit 1\n"),
            (
                "sing-box",
                "#!/bin/sh\n[ ! -e \"$MODDIR/fail-validation\" ]\n",
            ),
        ] {
            fs::write(root.join("bin").join(name), script).unwrap();
            fs::set_permissions(
                root.join("bin").join(name),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        let config = json!({"outbounds":[
            {"tag":"node-a","type":"socks","server":"127.0.0.1","server_port":1080},
            {"tag":"proxy","type":"selector","outbounds":["node-a","proxy-auto"],"default":"node-a"},
            {"tag":"proxy-auto","type":"urltest","outbounds":["node-a"],"url":"https://example.com/"},
            {"tag":"final","type":"selector","outbounds":["proxy"],"default":"proxy"},
            {"tag":"direct","type":"direct"},{"tag":"block","type":"block"},
            {"tag":"ai-grok","type":"selector","outbounds":["node-a"],"default":"node-a"}
        ],"route":{"rules":[{"protocol":"dns","action":"hijack-dns"},{"rule_set":["grok"],"outbound":"ai-grok"}],"rule_set":[{"tag":"grok"}],"final":"final"}});
        fs::write(
            root.join(".config/sing-box/config.json"),
            serde_json::to_string_pretty(&config).unwrap(),
        )
        .unwrap();
        Self { root }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_magicnet-cli"))
            .args(args)
            .env("MODDIR", &self.root)
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.join("bin").display()),
            )
            .output()
            .unwrap()
    }

    fn inspect(&self) -> Value {
        let output = self.run(&["--json", "routing", "inspect"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"]["policy"].clone()
    }

    fn payload(&self) -> Value {
        let mut policy = self.inspect();
        for key in ["source", "targets", "rule_sets", "protected_count"] {
            policy.as_object_mut().unwrap().remove(key);
        }
        for rule in policy["rules"].as_array_mut().unwrap() {
            rule.as_object_mut().unwrap().remove("available");
        }
        policy
    }

    fn save(&self, payload: &Value) -> Output {
        let path = self.root.join(".tmp/webui-payload/routing.json");
        fs::write(&path, serde_json::to_vec(payload).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        self.run(&["routing", "save-file", path.to_str().unwrap()])
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn saves_full_crud_policy_and_keeps_it_during_subscription_materialization() {
    let fixture = Fixture::new();
    let mut payload = fixture.payload();
    payload["groups"] = json!([{"tag":"ai-grok","type":"selector","members":["node-a","proxy-auto"],"default":"node-a"}]);
    payload["rules"] = json!([
        {"conditions":{"domain_suffix":["grok.com"]},"target":"ai-grok"},
        {"conditions":{"domain_suffix":["example.com"]},"target":"node-a"}
    ]);
    let saved = fixture.save(&payload);
    assert!(
        saved.status.success(),
        "{}",
        String::from_utf8_lossy(&saved.stderr)
    );
    assert_eq!(fixture.inspect()["source"], "user_intent");
    let config_path = fixture.root.join(".config/sing-box/config.json");
    let before: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    assert_eq!(before["route"]["rules"][0]["action"], "hijack-dns");
    let mut updated = before.clone();
    updated["outbounds"]
        .as_array_mut()
        .unwrap()
        .retain(|outbound| outbound["tag"] != "ai-grok");
    fs::write(&config_path, serde_json::to_vec(&updated).unwrap()).unwrap();
    let rendered = fixture.run(&["routing", "render", config_path.to_str().unwrap()]);
    assert!(
        rendered.status.success(),
        "{}",
        String::from_utf8_lossy(&rendered.stderr)
    );
    let after: Value = serde_json::from_slice(&fs::read(config_path).unwrap()).unwrap();
    assert_eq!(after["route"]["rules"], before["route"]["rules"]);
    assert!(after["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .any(|outbound| outbound["tag"] == "ai-grok"));
    let legacy = fixture.run(&[
        "route",
        "edit-domain",
        "node-a",
        "direct",
        "example.com",
        "edited.invalid",
    ]);
    assert!(
        legacy.status.success(),
        "{}",
        String::from_utf8_lossy(&legacy.stderr)
    );
    assert!(fixture.inspect()["rules"]
        .as_array()
        .unwrap()
        .iter()
        .any(|rule| rule["target"] == "direct"));
    assert!(fixture
        .run(&["route", "add-domain", "proxy", "manual.invalid"])
        .status
        .success());
    assert_eq!(fixture.inspect()["rules"][0]["target"], "proxy");
    assert!(fixture
        .run(&["route", "remove-domain", "proxy", "manual.invalid"])
        .status
        .success());
}

#[test]
fn validation_conflicts_and_failed_apply_do_not_leave_partial_user_intent() {
    for failure in ["fail-validation", "fail-apply", "stale-revision"] {
        let fixture = Fixture::new();
        let before = fs::read(fixture.root.join(".config/sing-box/config.json")).unwrap();
        let mut payload = fixture.payload();
        payload["rules"] =
            json!([{ "conditions":{"domain_suffix":["edited.invalid"]}, "target":"direct" }]);
        if failure == "stale-revision" {
            payload["revision"] = json!("0".repeat(64));
        } else {
            fs::write(fixture.root.join(failure), "1").unwrap();
        }
        assert!(!fixture.save(&payload).status.success());
        assert!(!fixture
            .root
            .join(".config/magicnet/routing-policy.json")
            .exists());
        let after: Value = serde_json::from_slice(
            &fs::read(fixture.root.join(".config/sing-box/config.json")).unwrap(),
        )
        .unwrap();
        let original: Value = serde_json::from_slice(&before).unwrap();
        assert_eq!(after, original);
        assert_eq!(
            before,
            fs::read(fixture.root.join(".config/sing-box/config.json")).unwrap()
        );
        assert!(!after["route"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule["domain_suffix"] == json!(["edited.invalid"])));
    }
}

#[test]
fn json_writes_are_rejected_without_touching_configuration() {
    let fixture = Fixture::new();
    let before = fs::read(fixture.root.join(".config/sing-box/config.json")).unwrap();
    let output = fixture.run(&["--json", "routing", "save-file", "/not/a/payload"]);
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["ok"], false);
    assert_eq!(
        before,
        fs::read(fixture.root.join(".config/sing-box/config.json")).unwrap()
    );
}

#[test]
fn failed_update_restores_existing_intent_and_config_byte_exactly() {
    let fixture = Fixture::new();
    let mut payload = fixture.payload();
    payload["rules"] =
        json!([{"conditions":{"domain_suffix":["first.invalid"]},"target":"node-a"}]);
    assert!(fixture.save(&payload).status.success());
    let config_path = fixture.root.join(".config/sing-box/config.json");
    let intent_path = fixture.root.join(".config/magicnet/routing-policy.json");
    let config_before = fs::read(&config_path).unwrap();
    let intent_before = fs::read(&intent_path).unwrap();
    payload = fixture.payload();
    payload["rules"] =
        json!([{"conditions":{"domain_suffix":["replacement.invalid"]},"target":"direct"}]);
    fs::write(fixture.root.join("fail-apply"), "1").unwrap();
    assert!(!fixture.save(&payload).status.success());
    assert_eq!(fs::read(config_path).unwrap(), config_before);
    assert_eq!(fs::read(intent_path).unwrap(), intent_before);
}

#[test]
fn saved_rules_with_disappeared_classifiers_can_be_deleted_and_repaired() {
    let fixture = Fixture::new();
    assert!(fixture.save(&fixture.payload()).status.success());
    let config_path = fixture.root.join(".config/sing-box/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["route"]["rule_set"] = json!([]);
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let mut payload = fixture.payload();
    assert_eq!(
        payload["rules"][0]["conditions"]["rule_set"],
        json!(["grok"])
    );
    payload["rules"] = json!([]);
    assert!(fixture.save(&payload).status.success());
    assert!(fixture.inspect()["rules"].as_array().unwrap().is_empty());
}

#[test]
fn migration_only_publishes_intent_with_active_config_not_subscription_candidates() {
    let fixture = Fixture::new();
    fs::write(
        fixture
            .root
            .join(".config/magicnet/selector-selections.json"),
        r#"{"ai-grok":"node-a"}"#,
    )
    .unwrap();
    let active = fixture.root.join(".config/sing-box/config.json");
    let candidate = fixture
        .root
        .join(".config/sing-box/subscription-candidate.json");
    fs::copy(&active, &candidate).unwrap();
    assert!(fixture
        .run(&["routing", "render", candidate.to_str().unwrap()])
        .status
        .success());
    assert!(!fixture
        .root
        .join(".config/magicnet/routing-policy.json")
        .exists());
    assert!(fixture
        .run(&["routing", "render", active.to_str().unwrap()])
        .status
        .success());
    assert_eq!(fixture.inspect()["source"], "user_intent");
    assert_eq!(fixture.inspect()["rules"][0]["target"], "node-a");
}
