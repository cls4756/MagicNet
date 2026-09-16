#!/usr/bin/env bash
# Domain forwarding contract: the sniffed domain only replaces the packet
# destination for TCP, only when the installed core advertises the fork option,
# and only while the switch is on. A core without the option must leave the
# configuration untouched instead of writing a key sing-box would reject.

set -Eeuo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/magicnet-domain-forward.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

need() {
    command -v "$1" >/dev/null 2>&1 || {
        printf 'missing required command: %s\n' "$1" >&2
        exit 127
    }
}
need jq

MODDIR="$WORK/module"
mkdir -p "$MODDIR/lib/magicnet" "$MODDIR/bin" "$MODDIR/.config/magicnet" "$MODDIR/.config/sing-box"
export MODDIR

# Load only the module under test. The rewrite is a pure config transformation;
# pulling in the aggregate shell surface would require a device.
# shellcheck disable=SC1091
. "$ROOT/src/MagicNet/lib/magicnet/domain_forward.sh"

magicnet_warn() { :; }

CONFIG="$MODDIR/.config/sing-box/config.json"
CONF="$MODDIR/.config/magicnet/domain-forward.conf"
BIN="$MODDIR/bin/sing-box"

fail() {
    printf 'FAIL: %s\n' "$1" >&2
    exit 1
}

write_base_config() {
    cat >"$CONFIG" <<'JSON'
{
  "route": {
    "rules": [
      {"inbound":["magicnet-dns-in"],"action":"hijack-dns"},
      {"inbound":["mixed-in","tun-in"],"action":"sniff"},
      {"domain_suffix":["example.invalid"],"outbound":"proxy"}
    ]
  }
}
JSON
    chmod 600 "$CONFIG"
}

core_capable() { printf 'binary-padding-override_destination-padding\n' >"$BIN"; }
core_incapable() { printf 'binary-padding-without-the-option\n' >"$BIN"; }

chmod_is_enforced() {
    local probe="$WORK/.chmod-probe"
    : >"$probe" && chmod 600 "$probe" 2>/dev/null
    [ "$(stat -c '%a' "$probe" 2>/dev/null)" = "600" ]
}
override_rule_count() {
    jq '[.route.rules[] | select((.action // "") == "sniff" and has("override_destination"))] | length' "$CONFIG"
}

assert_rule_shape() {
    jq -e '
      [.route.rules[] | select((.action // "") == "sniff" and has("override_destination"))] as $rules
      | ($rules | length) == 1
        and $rules[0].override_destination == true
        and $rules[0].network == ["tcp"]
        and $rules[0].inbound == ["tun-in"]
    ' "$CONFIG" >/dev/null || fail "domain forward rule shape is wrong: $(cat "$CONFIG")"
}

# --- default is on, and an explicit off value is the only way to disable ------

rm -f "$CONF"
magicnet_domain_forward_enabled || fail "domain forwarding must default to on"
for value in 0 false off no disabled DISABLED; do
    printf 'MAGICNET_DOMAIN_FORWARD=%s\n' "$value" >"$CONF"
    if magicnet_domain_forward_enabled; then
        fail "MAGICNET_DOMAIN_FORWARD=$value must disable domain forwarding"
    fi
done
printf 'MAGICNET_DOMAIN_FORWARD=1\n' >"$CONF"
magicnet_domain_forward_enabled || fail "MAGICNET_DOMAIN_FORWARD=1 must enable domain forwarding"
rm -f "$CONF"

# --- an incapable core must never see the option ------------------------------

core_incapable
if magicnet_domain_forward_supported; then
    fail "a core without the fork option must not be reported as supported"
fi
if magicnet_domain_forward_effective; then
    fail "domain forwarding must not be effective on an incapable core"
fi
write_base_config
before="$(jq -S -c . "$CONFIG")"
magicnet_singbox_apply_domain_forward "$CONFIG" || fail "apply must succeed as a no-op"
[ "$(jq -S -c . "$CONFIG")" = "$before" ] || fail "an incapable core must leave the config unchanged"
[ "$(override_rule_count)" = 0 ] || fail "an incapable core must not gain an override rule"

# --- a capable core plus the default switch adds exactly one TCP rule ---------

core_capable
magicnet_domain_forward_supported || fail "the capability probe missed the option"
magicnet_domain_forward_effective || fail "domain forwarding must be effective by default"

write_base_config
magicnet_singbox_apply_domain_forward "$CONFIG" || fail "apply failed on a capable core"
assert_rule_shape
jq -e '([.route.rules[] | select((.action // "") == "sniff" and (has("override_destination") | not))] | length) == 1' \
    "$CONFIG" >/dev/null || fail "the original sniff rule must survive the rewrite"
jq -e '[.route.rules[] | select((.action // "") == "sniff" and has("override_destination") and ((.network // []) | index("udp")))] | length == 0' \
    "$CONFIG" >/dev/null || fail "UDP must never be covered by the override rule"
jq -e '.route.rules[0].action == "hijack-dns"' "$CONFIG" >/dev/null ||
    fail "unrelated rules must keep their position"
jq -e '.route.rules[2].action == "sniff" and (.route.rules[2] | has("override_destination") | not)' \
    "$CONFIG" >/dev/null || fail "the override rule must precede the plain sniff rule"
# Hosts that cannot represent POSIX modes (a Windows checkout) must not turn the
# permission contract into a false failure, but CI still enforces it.
if chmod_is_enforced; then
    [ "$(stat -c '%a' "$CONFIG")" = "600" ] || fail "the config must stay private"
fi

# --- the rewrite is idempotent, even after the inbound list is normalized -----

once="$(cat "$CONFIG")"
magicnet_singbox_apply_domain_forward "$CONFIG" || fail "second apply failed"
[ "$(cat "$CONFIG")" = "$once" ] || fail "apply is not idempotent"
[ "$(override_rule_count)" = 1 ] || fail "repeated apply must not accumulate rules"

# `singbox_prepare_route_config` rewrites every sniff rule's inbound list. The
# rule must survive that normalization without being duplicated or lost.
jq '.route.rules |= map(if (.action // "") == "sniff" then .inbound = ["mixed-in","tun-in"] else . end)' \
    "$CONFIG" >"$CONFIG.normalized" && mv "$CONFIG.normalized" "$CONFIG" && chmod 600 "$CONFIG"
magicnet_singbox_apply_domain_forward "$CONFIG" || fail "apply failed after normalization"
assert_rule_shape
[ "$(override_rule_count)" = 1 ] || fail "normalized rules must not accumulate"

# --- turning the switch off removes the rule and the key ----------------------

printf 'MAGICNET_DOMAIN_FORWARD=0\n' >"$CONF"
magicnet_domain_forward_effective && fail "an explicit off value must not stay effective"
magicnet_singbox_apply_domain_forward "$CONFIG" || fail "disable failed"
[ "$(override_rule_count)" = 0 ] || fail "the override rule must be removed when disabled"
jq -e '[.. | objects | select(has("override_destination"))] | length == 0' "$CONFIG" >/dev/null ||
    fail "no override key may survive a disable"
jq -e '[.route.rules[] | select((.action // "") == "sniff")] | length == 1' "$CONFIG" >/dev/null ||
    fail "exactly the plain sniff rule must remain when disabled"

# Re-enabling restores exactly one rule.
rm -f "$CONF"
magicnet_singbox_apply_domain_forward "$CONFIG" || fail "re-enable failed"
assert_rule_shape

# --- hand-written rules are not mistaken for generated ones -------------------

cat >"$CONFIG" <<'JSON'
{
  "route": {
    "rules": [
      {"inbound":["tun-in"],"network":["tcp"],"action":"sniff"},
      {"inbound":["mixed-in","tun-in"],"action":"sniff"}
    ]
  }
}
JSON
chmod 600 "$CONFIG"
printf 'MAGICNET_DOMAIN_FORWARD=0\n' >"$CONF"
magicnet_singbox_apply_domain_forward "$CONFIG" || fail "apply failed on hand-written rules"
jq -e '[.route.rules[] | select((.action // "") == "sniff" and ((.network // []) == ["tcp"]))] | length == 1' \
    "$CONFIG" >/dev/null || fail "a hand-written tcp sniff rule must be preserved"

# --- a missing jq is survivable: the runtime apply must not fail ---------------

no_jq_bin="$WORK/no-jq"
mkdir -p "$no_jq_bin"
for tool in awk sed tr tail grep mv rm chmod stat; do
    tool_path="$(command -v "$tool" 2>/dev/null)" || continue
    case "$tool_path" in
    /*) ln -sf "$tool_path" "$no_jq_bin/$tool" ;;
    esac
done
write_base_config
before="$(jq -S -c . "$CONFIG")"
if (
    PATH="$no_jq_bin"
    export PATH
    command -v jq >/dev/null 2>&1
); then
    fail "the no-jq fixture still exposes jq on PATH"
fi
if ! (
    PATH="$no_jq_bin"
    export PATH
    # shellcheck disable=SC1091
    . "$ROOT/src/MagicNet/lib/magicnet/domain_forward.sh"
    magicnet_warn() { :; }
    magicnet_singbox_apply_domain_forward "$CONFIG"
); then
    fail "a missing jq must not fail the apply"
fi
[ "$(jq -S -c . "$CONFIG")" = "$before" ] ||
    fail "a missing jq must leave the config unchanged"

# --- a missing or absent config is not an error -------------------------------

rm -f "$CONFIG"
magicnet_singbox_apply_domain_forward "$CONFIG" || fail "a missing config must be a no-op"

printf 'domain forward tests passed\n'
