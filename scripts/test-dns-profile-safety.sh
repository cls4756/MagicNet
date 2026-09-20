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
mkdir -p "$MODDIR/.config/sing-box" "$MODDIR/bin"
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

for direct_profile in cloudflare-udp-direct; do
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
done

MAGICNET_BOOTSTRAP_DNS=aliyun MAGICNET_DNS_PROFILE=default magicnet_dns_apply_singbox
jq -e '
  .dns.final == "bootstrap-local-dns"
    and ([.dns.servers[] | select(.tag == "cloudflare-profile-dns" or .tag == "cloudflare-backup-dns")] | length) == 0
    and ([.dns.servers[] | select(.tag == "doh-cloudflare" or .tag == "doh-google")] | length) == 0
    and all(.dns.rules[]?; (.server == "bootstrap-local-dns") or (.server == "retained-dns"))
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
