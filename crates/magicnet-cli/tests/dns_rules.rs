#![cfg(any(target_os = "android", target_os = "linux"))]

use serde_json::{json, Value};
use std::fs;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "magicnet-dns-save-'quoted-{}-{nonce}",
            std::process::id()
        ));
        for directory in [
            "bin",
            "lib/kamfw",
            "lib/magicnet",
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
        ] {
            fs::write(root.join("lib/magicnet").join(name), text).unwrap();
        }
        fs::create_dir_all(root.join("lib/magicnet/singbox_subscribe")).unwrap();
        fs::write(
            root.join("lib/magicnet/singbox_subscribe/bootstrap.sh"),
            include_str!("../../../src/MagicNet/lib/magicnet/singbox_subscribe/bootstrap.sh"),
        )
        .unwrap();
        fs::write(root.join("lib/magicnet_singbox_subscribe.sh"), "magicnet_singbox_update_lock_active() { [ -d \"$MODDIR/.state/sing-box/subscription-update.lock\" ]; }\n").unwrap();
        fs::write(
            root.join("lib/magicnet.sh"),
            r#". "$MODDIR/lib/magicnet/common.sh"
magicnet_apply_runtime_config() {
    [ "${MAGICNET_CONFIG_LOCK_HELD:-0}" = 1 ] && [ -r "$MODDIR/.state/config.lock/pid" ] || return 1
    if env | grep -q '^MAGICNET_\(CONFIG_LOCK_HELD\|DNS_RULES_LOCKED\)='; then return 1; fi
    if [ -e "$MODDIR/fail-apply" ]; then
        rm "$MODDIR/fail-apply"
        printf '%s\n' 'partial-materialization' >"$MODDIR/.config/sing-box/config.json"
        return 1
    fi
    if grep -q edited.invalid "$MODDIR/.config/magicnet/dns-rules.json" 2>/dev/null; then
        cp "$MODDIR/expected.json" "$MODDIR/.config/sing-box/config.json"
    else
        cp "$MODDIR/original.json" "$MODDIR/.config/sing-box/config.json"
    fi
}
"#,
        )
        .unwrap();
        let validator = root.join("bin/sing-box");
        fs::write(
            &validator,
            r#"#!/bin/sh
config=
data=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -c) config=$2; shift 2 ;;
    -D) data=$2; shift 2 ;;
    *) shift ;;
  esac
done
[ -r "$config" ] && [ -r "$data/rules/required.srs" ] || exit 1
[ ! -e "$MODDIR/fail-validation" ]
"#,
        )
        .unwrap();
        fs::set_permissions(&validator, fs::Permissions::from_mode(0o755)).unwrap();
        for name in ["pidof", "pgrep"] {
            let path = root.join("bin").join(name);
            fs::write(&path, "#!/bin/sh\nexit 1\n").unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::write(
            root.join(".config/sing-box/rules/required.srs"),
            "fixture-relative-resource",
        )
        .unwrap();
        fs::write(
            root.join(".config/magicnet/dns.conf"),
            "MAGICNET_DNS_PROFILE=cloudflare-doh\n",
        )
        .unwrap();
        let original = json!({"dns": {
            "servers": [{"type":"udp","tag":"bootstrap-local-dns","server":"192.168.1.1"}, {"type":"https","tag":"cloudflare-profile-dns","server":"1.1.1.1"}],
            "rules": [{"clash_mode":"Global","server":"cloudflare-profile-dns"}, {"domain_suffix":["cn"],"server":"bootstrap-local-dns"}],
            "final":"cloudflare-profile-dns"
        }});
        fs::write(root.join("original.json"), original.to_string()).unwrap();
        fs::write(
            root.join(".config/sing-box/config.json"),
            original.to_string(),
        )
        .unwrap();
        let mut expected = original;
        expected["dns"]["rules"][1] =
            json!({"domain_suffix":["edited.invalid"],"server":"cloudflare-profile-dns"});
        fs::write(root.join("expected.json"), expected.to_string()).unwrap();
        Self { root }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_magicnet-cli"))
            .args(args)
            .env("MODDIR", &self.root)
            .env("MAGICNET_CONFIG_LOCK_TIMEOUT", "0")
            .env_remove("MAGICNET_DNS_RULES_LOCKED")
            .env_remove("MAGICNET_CONFIG_LOCK_HELD")
            .output()
            .unwrap()
    }

    fn stage(&self) -> PathBuf {
        let output = self.run(&["--json", "dns", "inspect"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let snapshot: Value = serde_json::from_slice(&output.stdout).unwrap();
        let payload = json!({"schema":1,"revision":snapshot["data"]["revision"],"rules":[
            {"id":1}, {"id":2,"conditions":{"domain_suffix":["edited.invalid"]},"target":"__magicnet_dns_profile__"}
        ]});
        let path = self.root.join(".tmp/webui-payload/candidate.json");
        fs::write(&path, payload.to_string()).unwrap();
        path
    }

    fn save(&self, path: &std::path::Path) -> Output {
        self.run(&["dns", "rules", "save-file", path.to_str().unwrap()])
    }
    fn config(&self) -> Vec<u8> {
        fs::read(self.root.join(".config/sing-box/config.json")).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn saves_under_both_locks_and_validates_relative_resources_with_private_payload_paths() {
    let fixture = Fixture::new();
    let path = fixture.stage();
    let output = fixture.save(&path);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let intent: Value = serde_json::from_slice(
        &fs::read(fixture.root.join(".config/magicnet/dns-rules.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(intent["rules"][1]["server"], "__magicnet_dns_profile__");
    assert_eq!(
        fixture.config(),
        fs::read(fixture.root.join("expected.json")).unwrap()
    );
    assert!(!fixture.root.join(".state/config.lock").exists());
}

#[test]
fn failed_application_restores_absent_or_existing_intent_and_original_config() {
    for previous in [
        None,
        Some(
            json!({"schema":1,"rules":[
                {"clash_mode":"Global","server":"__magicnet_dns_profile__"},
                {"domain_suffix":["cn"],"server":"bootstrap-local-dns"}
            ]})
            .to_string(),
        ),
    ] {
        let fixture = Fixture::new();
        let intent_path = fixture.root.join(".config/magicnet/dns-rules.json");
        if let Some(previous) = previous.as_deref() {
            fs::write(&intent_path, previous).unwrap();
        }
        let original = fixture.config();
        let path = fixture.stage();
        fs::write(fixture.root.join("fail-apply"), "1").unwrap();
        let output = fixture.save(&path);
        assert!(!output.status.success());
        assert!(!fixture.root.join("fail-apply").exists());
        assert_eq!(fs::read_to_string(&intent_path).ok(), previous);
        assert_eq!(fixture.config(), original);
        assert!(!fixture.root.join(".state/config.lock").exists());
    }
}

#[test]
fn stale_validation_and_transaction_conflicts_do_not_overwrite_config_or_intent() {
    for failure in [
        "stale",
        "validation",
        "subscription",
        "transparent",
        "config-lock",
    ] {
        let fixture = Fixture::new();
        let path = fixture.stage();
        match failure {
            "stale" => {
                fs::write(
                    fixture.root.join(".config/magicnet/dns.conf"),
                    "MAGICNET_DNS_PROFILE=default\n",
                )
                .unwrap();
            }
            "validation" => {
                fs::write(fixture.root.join("fail-validation"), "1").unwrap();
            }
            "subscription" => {
                fs::create_dir_all(
                    fixture
                        .root
                        .join(".state/sing-box/subscription-update.lock"),
                )
                .unwrap();
            }
            "transparent" => {
                fs::create_dir_all(fixture.root.join(".state/transparent-transaction")).unwrap();
            }
            "config-lock" => {
                fs::create_dir_all(fixture.root.join(".state/config.lock")).unwrap();
                fs::write(
                    fixture.root.join(".state/config.lock/pid"),
                    format!("{}\n", std::process::id()),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let original = fixture.config();
        let output = fixture.save(&path);
        assert!(!output.status.success(), "failure={failure}");
        assert_eq!(fixture.config(), original, "failure={failure}");
        assert!(
            !fixture
                .root
                .join(".config/magicnet/dns-rules.json")
                .exists(),
            "failure={failure}"
        );
    }
}

#[test]
fn config_editor_commits_obey_the_same_lifecycle_lock_as_dns_save() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.root.join(".state")).unwrap();
    let lock = fs::File::create(fixture.root.join(".state/config-apply.lock")).unwrap();
    assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) }, 0);
    let path = fixture.root.join(".tmp/webui-payload/full-config.json");
    fs::copy(fixture.root.join("expected.json"), &path).unwrap();
    let before = fixture.config();
    let output = fixture.run(&[
        "config-editor",
        "save-file",
        "sing-box",
        path.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert_eq!(fixture.config(), before);
    assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_UN) }, 0);
    let output = fixture.run(&[
        "config-editor",
        "save-file",
        "sing-box",
        path.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_ne!(fixture.config(), before);
}

#[test]
fn json_write_request_is_rejected_before_creating_lock_or_intent() {
    let fixture = Fixture::new();
    let path = fixture.stage();
    let output = fixture.run(&[
        "--json",
        "dns",
        "rules",
        "save-file",
        path.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["error"]["code"], "machine.unsupported_command");
    assert!(!fixture.root.join(".state").exists());
    assert!(!fixture
        .root
        .join(".config/magicnet/dns-rules.json")
        .exists());
}
