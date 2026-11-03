#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/magicnet-dns-profile.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

with_routing_assets=0
case "${1:-}" in
"") ;;
--with-routing-assets)
  with_routing_assets=1
  shift
  ;;
*)
  printf 'usage: bash scripts/test-dns-profile-safety.sh [--with-routing-assets]\n' >&2
  exit 64
  ;;
esac
[ "$#" -eq 0 ] || {
  printf 'unexpected arguments\n' >&2
  exit 64
}

MODDIR="$WORK/module"
export MODDIR
mkdir -p "$MODDIR/.config/sing-box" "$MODDIR/.config/magicnet" "$MODDIR/bin"
ln -s "$(command -v jq)" "$MODDIR/bin/jq"

cat >"$MODDIR/.config/sing-box/config.json" <<'EOF'
{
  "dns": {
    "servers": [
      {"type": "https", "tag": "bootstrap-local-dns", "server": "223.5.5.5"},
      {"type": "https", "tag": "cloudflare-backup-dns", "server": "1.0.0.1"},
      {"type": "https", "tag": "doh-cloudflare", "server": "1.1.1.1", "detour": "proxy"},
      {"type": "udp", "tag": "retained-udp", "server": "9.9.9.9"}
    ],
    "rules": [
      {"clash_mode": "Global", "server": "doh-cloudflare"},
      {"domain_suffix": ["cn"], "server": "bootstrap-local-dns"},
      {"rule_set": ["foreign"], "server": "doh-google"},
      {"domain_suffix": ["example.invalid"], "server": "retained-dns"}
    ]
  }
}
EOF

# shellcheck disable=SC1091
. "$ROOT/src/MagicNet/lib/magicnet/primitives.sh"
. "$ROOT/src/MagicNet/lib/magicnet/subscribe_bootstrap.sh"
. "$ROOT/src/MagicNet/lib/magicnet/dns.sh"
magicnet_conf_value() (
  awk -F= -v key="$2" '$1 == key { value = substr($0, index($0, "=") + 1); count++ } END { if (count == 1) print value; else exit 1 }' "$1"
)

assert_application_rule_roundtrip() (
  mkdir -p "$MODDIR/.config/sing-box" "$MODDIR/.config/magicnet" "$MODDIR/bin"
  ln -s "$(command -v jq)" "$MODDIR/bin/jq"
  cat >"$MODDIR/.config/sing-box/config.json" <<'EOF'
{"dns":{"servers":[
  {"type":"udp","tag":"bootstrap-local-dns","server":"192.168.50.1"},
  {"type":"https","tag":"doh-cloudflare","server":"1.1.1.1"},
  {"type":"udp","tag":"retained-dns","server":"192.0.2.53"}
],"rules":[
  {"clash_mode":"Global","server":"doh-cloudflare"},
  {"domain_suffix":["cn"],"server":"bootstrap-local-dns"},
  {"rule_set":["foreign"],"server":"doh-cloudflare"},
  {"domain_suffix":["retained.invalid"],"server":"retained-dns"}
]},"route":{"default_domain_resolver":{"server":"bootstrap-local-dns"}}}
EOF
  cat >"$MODDIR/.config/magicnet/dns.conf" <<'EOF'
MAGICNET_LAN_DNS_SERVER=192.168.50.1
MAGICNET_LAN_DNS_DOMAINS=router.lan
EOF
  printf '%s\n' '{"schema":1,"server":"resolver.example","server_port":443,"path":"/dns-query","via_proxy":true}' \
    >"$MODDIR/.config/magicnet/dns-custom.json"
  for selected in cloudflare-doh default cloudflare-dot default cloudflare-udp default custom-doh default google-dot default; do
    MAGICNET_DNS_PROFILE="$selected" MAGICNET_BOOTSTRAP_DNS=aliyun magicnet_dns_apply_singbox
    expected="${selected%%-*}-profile-dns"
    final="$expected"
    if [ "$selected" = default ]; then
      expected="default-remote-dns"
      final="bootstrap-local-dns"
    fi
    jq -e --arg expected "$expected" --arg final "$final" '
      .dns.final == $final and (.dns.rules | length) == 5
      and .dns.rules[0].server == "magicnet-lan-dns"
      and .dns.rules[1].server == $expected
      and .dns.rules[2].server == "bootstrap-local-dns"
      and .dns.rules[3].server == $expected
      and .dns.rules[4].server == "retained-dns"
      and .route.default_domain_resolver.server == "bootstrap-local-dns"
      and any(.dns.servers[]; .tag == $expected)
    ' "$MODDIR/.config/sing-box/config.json" >/dev/null || {
      printf 'application DNS rule identity was lost during roundtrip through %s\n' "$selected" >&2
      exit 1
    }
    [ ! -e "$MODDIR/.config/magicnet/dns-rules.json" ]
  done
  for bootstrap in aliyun baidu tencent; do
    MAGICNET_DNS_PROFILE=default MAGICNET_BOOTSTRAP_DNS="$bootstrap" magicnet_dns_apply_singbox
    jq -e '
      [.dns.servers[] | select(.tag == "default-remote-dns") | del(.tag)] ==
      [.dns.servers[] | select(.tag == "bootstrap-local-dns") | del(.tag)]
    ' "$MODDIR/.config/sing-box/config.json" >/dev/null
    before="$(sha256sum "$MODDIR/.config/sing-box/config.json" | awk '{print $1}')"
    MAGICNET_DNS_PROFILE=default MAGICNET_BOOTSTRAP_DNS="$bootstrap" magicnet_dns_apply_singbox
    [ "$before" = "$(sha256sum "$MODDIR/.config/sing-box/config.json" | awk '{print $1}')" ]
  done
  jq '.dns.rules |= map(if .server == "default-remote-dns" then .server = "bootstrap-local-dns" else . end)' \
    "$MODDIR/.config/sing-box/config.json" >"$WORK/legacy-rules.json"
  mv "$WORK/legacy-rules.json" "$MODDIR/.config/sing-box/config.json"
  MAGICNET_DNS_PROFILE=cloudflare-doh magicnet_dns_apply_singbox
  jq -e '
    .dns.rules[1].server == "bootstrap-local-dns"
    and .dns.rules[2].server == "bootstrap-local-dns"
    and .dns.rules[3].server == "bootstrap-local-dns"
    and .dns.final == "cloudflare-profile-dns"
  ' "$MODDIR/.config/sing-box/config.json" >/dev/null
  before="$(sha256sum "$MODDIR/.config/sing-box/config.json" | awk '{print $1}')"
  mv() { return 1; }
  if MAGICNET_DNS_PROFILE=default magicnet_dns_apply_singbox >/dev/null 2>&1; then
    printf 'failed DNS profile publication must be reported\n' >&2
    exit 1
  fi
  [ "$before" = "$(sha256sum "$MODDIR/.config/sing-box/config.json" | awk '{print $1}')" ]
  [ ! -e "$MODDIR/.config/sing-box/config.json.magicnet-dns.new" ]
)

MODDIR="$WORK/roundtrip" assert_application_rule_roundtrip

assert_profile_normalization() {
  local configured="$1"
  local expected="$2"
  [ "$(MAGICNET_DNS_PROFILE="$configured" magicnet_dns_profile)" = "$expected" ] || {
    printf 'DNS profile normalization failed for %s\n' "$configured" >&2
    exit 1
  }
}
for profile in cloudflare-dot cloudflare-dot-direct cloudflare-udp cloudflare-udp-direct; do
  assert_profile_normalization "$profile" "$profile"
done
assert_profile_normalization dot cloudflare-dot
assert_profile_normalization udp cloudflare-udp
assert_profile_normalization 1.1.1.1 cloudflare-udp

normalized_system_dns="$(printf '%s\n' \
  '192.168.50.1, /192.168.50.2 [2001:db8::53], [fe80::1%wlan0]' \
  '127.0.0.1 :: ::1 192.168.50.1' |
  magicnet_dns_normalize_server_addresses)"
[ "$normalized_system_dns" = "$(printf '%s\n' 192.168.50.1 192.168.50.2 2001:db8::53 fe80::1)" ] || {
  printf 'system DNS address normalization failed:\n%s\n' "$normalized_system_dns" >&2
  exit 1
}
if grep -Fq 'gsub(/^[\[/]+' "$ROOT/src/MagicNet/lib/magicnet/dns.sh"; then
  printf 'system DNS normalization must remain compatible with Android awk\n' >&2
  exit 1
fi

assert_bootstrap_server() {
  local bootstrap="$1"
  local expected_type="$2"
  local expected_server="$3"
  local expected_sni="${4:-}"
  MAGICNET_BOOTSTRAP_DNS="$bootstrap" MAGICNET_DNS_PROFILE=default magicnet_dns_apply_singbox
  jq -e --arg expected_type "$expected_type" --arg expected_server "$expected_server" \
    --arg expected_sni "$expected_sni" --argjson mark "$(magicnet_dns_capture_singbox_mark)" '
      [.dns.servers[] | select(.tag == "bootstrap-local-dns")] ==
      [if $expected_type == "udp" then
         {"type":"udp","tag":"bootstrap-local-dns","server":$expected_server,"routing_mark":$mark}
       else
         {"type":"https","tag":"bootstrap-local-dns","server":$expected_server,"server_port":443,"routing_mark":$mark,"path":"/dns-query","headers":{"Host":$expected_sni},"tls":{"server_name":$expected_sni}}
       end]
      and .dns.final == "bootstrap-local-dns"
    ' "$MODDIR/.config/sing-box/config.json" >/dev/null || {
    printf 'bootstrap DNS %s was not materialized as expected\n' "$bootstrap" >&2
    exit 1
  }
}

assert_bootstrap_server aliyun https 223.6.6.6 dns.alidns.com
assert_bootstrap_server baidu udp 180.76.76.76
assert_bootstrap_server tencent https 1.12.12.12 doh.pub
MAGICNET_ANDROID_SYSTEM_DNS_SERVERS='192.168.50.1, 2001:db8::53' \
  assert_bootstrap_server system udp 192.168.50.1

before_missing_system="$(sha256sum "$MODDIR/.config/sing-box/config.json" | awk '{print $1}')"
if MAGICNET_ANDROID_SYSTEM_DNS_SERVERS='127.0.0.1' MAGICNET_BOOTSTRAP_DNS=system \
  MAGICNET_DNS_PROFILE=default magicnet_dns_apply_singbox >/dev/null 2>&1; then
  printf 'system bootstrap must fail when Android exposes no usable DNS server\n' >&2
  exit 1
fi
after_missing_system="$(sha256sum "$MODDIR/.config/sing-box/config.json" | awk '{print $1}')"
[ "$before_missing_system" = "$after_missing_system" ] || {
  printf 'failed system bootstrap discovery must not modify sing-box config\n' >&2
  exit 1
}

assert_profile_uses_proxy_detour() {
  local profile="$1"
  local expected_type="$2"
  local expected_port="$3"
  local tag_prefix="$4"
  MAGICNET_BOOTSTRAP_DNS=aliyun MAGICNET_DNS_PROFILE="$profile" magicnet_dns_apply_singbox

  jq -e --arg expected_type "$expected_type" --argjson expected_port "$expected_port" --arg tag_prefix "$tag_prefix" '
      ([.dns.servers[]
        | select(.tag == ($tag_prefix + "-profile-dns") or .tag == ($tag_prefix + "-backup-dns"))
        | select(.type == $expected_type and .detour == "proxy")
        | select((.server_port // 53) == $expected_port)] | length) == 2
        and .dns.final == ($tag_prefix + "-profile-dns")
        and all(.dns.rules[]?; (.server == ($tag_prefix + "-profile-dns")) or (.server == "bootstrap-local-dns") or (.server == "retained-dns"))
        and ([.dns.servers[] | select(.tag == "doh-cloudflare" or .tag == "doh-google")] | length) == 0
        and ([.dns.servers[] | select(.tag == "bootstrap-local-dns")
          | .type == "https" and .server == "223.5.5.5" and has("detour") | not] | length) == 1
    ' "$MODDIR/.config/sing-box/config.json" >/dev/null || {
    printf 'DNS profile %s must use proxy detour for both managed %s servers\n' \
      "$profile" "$expected_type" >&2
    exit 1
  }
}

# -direct profiles must contact servers directly (no detour:"proxy"), with UDP
# servers marked with a routing_mark for kernel-bypass exemption.
assert_profile_direct_only() {
  local profile="$1"
  local expected_type="$2"
  local expected_port="$3"
  local tag_prefix="$4"
  MAGICNET_BOOTSTRAP_DNS=aliyun MAGICNET_DNS_PROFILE="$profile" magicnet_dns_apply_singbox

  jq -e --arg expected_type "$expected_type" --argjson expected_port "$expected_port" --arg tag_prefix "$tag_prefix" --argjson mark "$(magicnet_dns_capture_singbox_mark)" '
      ([.dns.servers[]
        | select(.tag == ($tag_prefix + "-profile-dns") or .tag == ($tag_prefix + "-backup-dns"))
        | select(.type == $expected_type and (.detour // "") == "")
        | select((.server_port // 53) == $expected_port)] | length) == 2
        and .dns.final == ($tag_prefix + "-profile-dns")
        and all(.dns.rules[]?; (.server == ($tag_prefix + "-profile-dns")) or (.server == "bootstrap-local-dns") or (.server == "retained-dns"))
        and ([.dns.servers[] | select(.tag == "doh-cloudflare" or .tag == "doh-google")] | length) == 0
        and ([.dns.servers[]
          | select(.tag == ($tag_prefix + "-profile-dns") or .tag == ($tag_prefix + "-backup-dns"))
          | select(.type == "udp") | .routing_mark == $mark] | length) == (if $expected_type == "udp" then 2 else 0 end)
        and ([.dns.servers[] | select(.tag == "bootstrap-local-dns")
          | .type == "https" and (has("detour") | not)] | length) == 1
    ' "$MODDIR/.config/sing-box/config.json" >/dev/null || {
    printf 'DNS profile %s must contact servers directly without proxy detour\n' "$profile" >&2
    exit 1
  }
}

assert_profile_uses_proxy_detour cloudflare-udp udp 53 cloudflare
assert_profile_uses_proxy_detour cloudflare-dot tls 853 cloudflare
assert_profile_uses_proxy_detour cloudflare-doh https 443 cloudflare
assert_profile_uses_proxy_detour google-doh https 443 google
assert_profile_uses_proxy_detour google-dot tls 853 google
assert_profile_uses_proxy_detour adguard-doh https 443 adguard
assert_profile_uses_proxy_detour quad9-doh https 443 quad9

MAGICNET_BOOTSTRAP_DNS=aliyun MAGICNET_DNS_VIA_PROXY=0 MAGICNET_DNS_PROFILE=cloudflare-doh magicnet_dns_apply_singbox
jq -e '
  ([.dns.servers[]
    | select(.tag == "cloudflare-profile-dns" or .tag == "cloudflare-backup-dns")
    | select((.detour // "") == "" and .type == "https")] | length) == 2
' "$MODDIR/.config/sing-box/config.json" >/dev/null || {
  printf 'MAGICNET_DNS_VIA_PROXY=0 must force profile DNS direct\n' >&2
  exit 1
}

# -direct profiles must NOT use proxy detour — they contact servers directly.
assert_profile_direct_only cloudflare-udp-direct udp 53 cloudflare
assert_profile_direct_only cloudflare-dot-direct tls 853 cloudflare
assert_profile_direct_only cloudflare-doh-direct https 443 cloudflare
assert_profile_direct_only google-doh-direct https 443 google
assert_profile_direct_only google-dot-direct tls 853 google
assert_profile_direct_only adguard-doh-direct https 443 adguard
assert_profile_direct_only quad9-doh-direct https 443 quad9

direct_profile=cloudflare-udp-direct
  MAGICNET_BOOTSTRAP_DNS=aliyun MAGICNET_DNS_PROFILE="$direct_profile" magicnet_dns_apply_singbox
  jq -e --arg direct_profile "$direct_profile" '
    (if ($direct_profile | startswith("cloudflare")) then "cloudflare" else "google" end) as $tag_prefix
    | ([.dns.servers[]
        | select(.tag == ($tag_prefix + "-profile-dns") or .tag == ($tag_prefix + "-backup-dns"))
        | select(.type == "udp" and (.detour // "") == "")]
       | length) == 2
    and .dns.final == ($tag_prefix + "-profile-dns")
  ' "$MODDIR/.config/sing-box/config.json" >/dev/null || {
    printf 'DNS profile %s must preserve UDP transport when direct\n' "$direct_profile" >&2
    exit 1
  }

cat >"$MODDIR/.config/magicnet/dns.conf" <<'EOF'
MAGICNET_LAN_DNS_SERVER=192.168.50.1
MAGICNET_LAN_DNS_DOMAINS=router.lan,nas.home.arpa
EOF
MAGICNET_BOOTSTRAP_DNS=aliyun MAGICNET_DNS_PROFILE=cloudflare-doh magicnet_dns_apply_singbox
jq -e --argjson mark "$(magicnet_dns_capture_singbox_mark)" '
  ([.dns.servers[] | select(.tag == "magicnet-lan-dns")]
    == [{"type":"udp","tag":"magicnet-lan-dns","server":"192.168.50.1","server_port":53,"routing_mark":$mark}])
  and .dns.rules[0] == {"domain_suffix":["router.lan","nas.home.arpa"],"server":"magicnet-lan-dns"}
  and .dns.final == "cloudflare-profile-dns"
' "$MODDIR/.config/sing-box/config.json" >/dev/null || {
  printf 'custom LAN DNS must be materialized before public DNS rules\n' >&2
  exit 1
}
rm -f "$MODDIR/.config/magicnet/dns.conf"
MAGICNET_BOOTSTRAP_DNS=aliyun MAGICNET_DNS_PROFILE=default magicnet_dns_apply_singbox
jq -e '
  .dns.final == "bootstrap-local-dns"
    and ([.dns.servers[] | select(.tag == "cloudflare-profile-dns" or .tag == "cloudflare-backup-dns")] | length) == 0
    and ([.dns.servers[] | select(.tag == "doh-cloudflare" or .tag == "doh-google")] | length) == 0
    and all(.dns.rules[]?; (.server == "default-remote-dns") or (.server == "bootstrap-local-dns") or (.server == "retained-dns"))
    and ([.dns.servers[] | select(.tag == "retained-udp") | .routing_mark] == [1073741824])
    and .dns.timeout == "8s"
    and .dns.cache_capacity == 4096
    and .dns.optimistic == {"enabled": true, "timeout": "30m"}
    and .experimental.cache_file.enabled == true
    and .experimental.cache_file.store_dns == true
' "$MODDIR/.config/sing-box/config.json" >/dev/null || {
  printf 'default DNS profile must restore direct bootstrap and conservative sing-box 1.14 cache defaults\n' >&2
  exit 1
}

cat >"$MODDIR/.config/sing-box/config.json" <<'EOF'
{
  "dns": {
    "servers": [
      {"type": "https", "tag": "bootstrap-local-dns", "server": "223.5.5.5"},
      {"type": "udp", "tag": "retained-udp", "server": "9.9.9.9"}
    ],
    "timeout": "12s",
    "cache_capacity": 2048,
    "optimistic": false
  },
  "experimental": {
    "cache_file": {
      "enabled": false
    }
  }
}
EOF
MAGICNET_BOOTSTRAP_DNS=aliyun MAGICNET_DNS_PROFILE=default magicnet_dns_apply_singbox
jq -e '
  .dns.timeout == "12s"
    and .dns.cache_capacity == 2048
    and .dns.optimistic == false
    and .experimental.cache_file.enabled == false
    and (.experimental.cache_file | has("store_dns") | not)
' "$MODDIR/.config/sing-box/config.json" >/dev/null || {
  printf 'explicit DNS cache and timeout preferences must be preserved\n' >&2
  exit 1
}

mkdir -p "$MODDIR/.config/magicnet"
cat >"$MODDIR/.config/magicnet/dns-rules.json" <<'EOF'
{"schema":1,"rules":[
  {"clash_mode":"Global","server":"__magicnet_dns_profile__"},
  {"domain_suffix":["cn"],"server":"bootstrap-local-dns"},
  {"domain_suffix":["edited.invalid"],"server":"__magicnet_dns_profile__"}
]}
EOF
jq '.dns.servers += [{"type":"tailscale","tag":"tailscale-dns"}] | .dns.rules = [{"domain_suffix":["feature.invalid"],"server":"tailscale-dns"}] + .dns.rules' \
  "$MODDIR/.config/sing-box/config.json" >"$WORK/with-feature.json"
mv "$WORK/with-feature.json" "$MODDIR/.config/sing-box/config.json"
cat >"$MODDIR/.config/magicnet/dns.conf" <<'EOF'
MAGICNET_LAN_DNS_SERVER=192.168.50.1
MAGICNET_LAN_DNS_DOMAINS=router.lan,nas.home.arpa
EOF
for profile in cloudflare-doh default google-dot default cloudflare-doh; do
  MAGICNET_DNS_PROFILE="$profile" MAGICNET_BOOTSTRAP_DNS=aliyun magicnet_dns_apply_singbox
  expected="default-remote-dns"
  [ "$profile" = default ] || expected="${profile%%-*}-profile-dns"
  jq -e --arg expected "$expected" '
    (.dns.rules | length) == 5
    and .dns.rules[0] == {"domain_suffix":["router.lan","nas.home.arpa"],"server":"magicnet-lan-dns"}
    and .dns.rules[1].server == "tailscale-dns"
    and .dns.rules[2].server == $expected
    and .dns.rules[3].server == "bootstrap-local-dns"
    and .dns.rules[4].domain_suffix == ["edited.invalid"]
    and .dns.rules[4].server == $expected
  ' "$MODDIR/.config/sing-box/config.json" >/dev/null
done
rm "$MODDIR/.config/magicnet/dns.conf"
MAGICNET_DNS_PROFILE=cloudflare-doh magicnet_dns_apply_singbox
jq -e '
  (.dns.rules | length) == 4
  and all(.dns.rules[]; .server != "magicnet-lan-dns")
  and .dns.rules[3].domain_suffix == ["edited.invalid"]
' "$MODDIR/.config/sing-box/config.json" >/dev/null

jq -n '{schema:1,rules:[range(0;128) | {domain_suffix:[range(0;32) | (("x" * 160) + ".invalid")],server:"__magicnet_dns_profile__"}]}' \
  >"$MODDIR/.config/magicnet/dns-rules.json"
MAGICNET_DNS_PROFILE=cloudflare-doh magicnet_dns_apply_singbox
jq -e '(.dns.rules | length) == 129 and .dns.rules[128].server == "cloudflare-profile-dns"' \
  "$MODDIR/.config/sing-box/config.json" >/dev/null

before_invalid_intent="$(sha256sum "$MODDIR/.config/sing-box/config.json" | awk '{print $1}')"
for malformed in 'invalid' '{"schema":2,"rules":[]}' '{"schema":1,"rules":[null]}'; do
  printf '%s\n' "$malformed" >"$MODDIR/.config/magicnet/dns-rules.json"
  if MAGICNET_DNS_PROFILE=default magicnet_dns_apply_singbox >/dev/null 2>&1; then
    printf 'malformed DNS rule intent must be rejected\n' >&2
    exit 1
  fi
  [ "$before_invalid_intent" = "$(sha256sum "$MODDIR/.config/sing-box/config.json" | awk '{print $1}')" ]
done
rm "$MODDIR/.config/magicnet/dns-rules.json"
ln -s "$MODDIR/.config/sing-box/config.json" "$MODDIR/.config/magicnet/dns-rules.json"
if MAGICNET_DNS_PROFILE=default magicnet_dns_apply_singbox >/dev/null 2>&1; then
  printf 'symlink DNS rule intent must be rejected\n' >&2
  exit 1
fi
[ "$before_invalid_intent" = "$(sha256sum "$MODDIR/.config/sing-box/config.json" | awk '{print $1}')" ]
rm "$MODDIR/.config/magicnet/dns-rules.json"

printf '%s\n' '{"schema":1,"rules":[{"domain_suffix":["cn"],"server":"bootstrap-local-dns"},{"domain_suffix":["example"],"server":"__magicnet_dns_profile__"}]}' >"$MODDIR/.config/magicnet/dns-rules.json"
for via_proxy in true false; do
  jq -n --argjson via_proxy "$via_proxy" '{schema:1,server:"resolver.example",server_port:8443,path:"/private/dns?key=fixture",via_proxy:$via_proxy}' >"$MODDIR/.config/magicnet/dns-custom.json"
  MAGICNET_DNS_PROFILE=custom-doh magicnet_dns_apply_singbox
  jq -e --argjson via_proxy "$via_proxy" '
    .dns.final == "custom-profile-dns"
    and ([.dns.servers[] | select(.tag == "custom-profile-dns")][0]
      | .server == "resolver.example" and .server_port == 8443 and .path == "/private/dns?key=fixture"
        and .tls.server_name == "resolver.example" and (.tls.insecure // false) == false
        and .domain_resolver.server == "bootstrap-local-dns"
        and (if $via_proxy then .detour == "proxy" and (has("routing_mark") | not)
          else .routing_mark == 1073741824 and (has("detour") | not) end))
    and any(.dns.servers[]; .tag == "retained-udp")
    and any(.dns.rules[]; .server == "custom-profile-dns")
    and any(.dns.rules[]; .server == "bootstrap-local-dns")
  ' "$MODDIR/.config/sing-box/config.json" >/dev/null
done
for profile in cloudflare-doh default custom-doh; do
  MAGICNET_DNS_PROFILE="$profile" magicnet_dns_apply_singbox
  jq -e --arg profile "$profile" '
    if $profile == "custom-doh" then .dns.final == "custom-profile-dns"
    else all(.dns.servers[]; .tag != "custom-profile-dns") end
  ' "$MODDIR/.config/sing-box/config.json" >/dev/null
done
for server in '2001:db8::1' '::ffff:192.0.2.1'; do
  jq --arg server "$server" '.server=$server | .server_port=443' "$MODDIR/.config/magicnet/dns-custom.json" >"$WORK/ipv6.json"
  mv "$WORK/ipv6.json" "$MODDIR/.config/magicnet/dns-custom.json"
  MAGICNET_DNS_PROFILE=custom-doh magicnet_dns_apply_singbox
  jq -e --arg server "$server" 'any(.dns.servers[]; .tag == "custom-profile-dns" and .server == $server and .headers.Host == ("[" + $server + "]"))' "$MODDIR/.config/sing-box/config.json" >/dev/null
done
before_invalid_custom="$(sha256sum "$MODDIR/.config/sing-box/config.json" | awk '{print $1}')"
for malformed in 'invalid' '{"schema":2}' '{"schema":1,"server":"dns.example","server_port":0,"path":"/dns-query","via_proxy":true}' '{"schema":1,"server":"dns.example","server_port":443,"path":"/dns-query%0d%0a","via_proxy":true}'; do
  printf '%s\n' "$malformed" >"$MODDIR/.config/magicnet/dns-custom.json"
  if MAGICNET_DNS_PROFILE=custom-doh magicnet_dns_apply_singbox >/dev/null 2>&1; then
    printf 'invalid custom DoH must be rejected\n' >&2
    exit 1
  fi
  [ "$before_invalid_custom" = "$(sha256sum "$MODDIR/.config/sing-box/config.json" | awk '{print $1}')" ]
done
rm "$MODDIR/.config/magicnet/dns-custom.json"
if MAGICNET_DNS_PROFILE=custom-doh magicnet_dns_apply_singbox >/dev/null 2>&1; then
  printf 'missing custom DoH must be rejected\n' >&2
  exit 1
fi
ln -s "$MODDIR/.config/sing-box/config.json" "$MODDIR/.config/magicnet/dns-custom.json"
if MAGICNET_DNS_PROFILE=custom-doh magicnet_dns_apply_singbox >/dev/null 2>&1; then
  printf 'symlink custom DoH must be rejected\n' >&2
  exit 1
fi
rm "$MODDIR/.config/magicnet/dns-custom.json"

if [ "$with_routing_assets" -eq 1 ]; then
  command -v sing-box >/dev/null 2>&1 || {
    printf 'prepared DNS checks require sing-box and rule-set assets\n' >&2
    exit 127
  }
  FULL_MODDIR="$WORK/full-module"
  mkdir -p "$FULL_MODDIR/.config/sing-box" "$FULL_MODDIR/bin"
  ln -s "$(command -v jq)" "$FULL_MODDIR/bin/jq"
  cp "$ROOT/src/MagicNet/.config/sing-box/config.json" "$FULL_MODDIR/.config/sing-box/config.json"
  cp -R "$ROOT/src/MagicNet/.config/sing-box/rules" "$FULL_MODDIR/.config/sing-box/"
  MODDIR="$FULL_MODDIR" MAGICNET_BOOTSTRAP_DNS=aliyun MAGICNET_DNS_PROFILE=cloudflare-udp magicnet_dns_apply_singbox
  (cd "$FULL_MODDIR/.config/sing-box" && sing-box check -c config.json -D "$FULL_MODDIR/.config/sing-box") >/dev/null
else
  printf 'Prepared sing-box DNS asset check excluded; use --with-routing-assets to include it.\n'
fi

printf 'DNS profile safety test passed\n'
