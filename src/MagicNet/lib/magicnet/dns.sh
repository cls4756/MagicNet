magicnet_dns_conf() {
    printf '%s\n' "${MODDIR}/.config/magicnet/dns.conf"
}

magicnet_dns_profile() {
    _profile="${MAGICNET_DNS_PROFILE:-}"
    if [ -z "$_profile" ]; then
        _profile="$(magicnet_conf_value "$(magicnet_dns_conf)" MAGICNET_DNS_PROFILE 2>/dev/null || true)"
    fi
    case "${_profile:-default}" in
    default) printf '%s\n' "default" ;;
    cloudflare-doh | cloudflare-doh-direct) printf '%s\n' "${_profile:-default}" ;;
    cloudflare-dot | cloudflare-dot-direct) printf '%s\n' "${_profile:-default}" ;;
    cloudflare-udp | cloudflare-udp-direct) printf '%s\n' "${_profile:-default}" ;;
    cloudflare | doh) printf '%s\n' "cloudflare-doh" ;;
    cloudflare-dot | dot) printf '%s\n' "cloudflare-dot" ;;
    cloudflare-udp | udp | 1.1.1.1) printf '%s\n' "cloudflare-udp" ;;
    google-doh | google-doh-direct | google) printf '%s\n' "${_profile:-google-doh}" ;;
    google-dot | google-dot-direct) printf '%s\n' "${_profile:-google-dot}" ;;
    adguard-doh | adguard-doh-direct | adguard) printf '%s\n' "${_profile:-adguard-doh}" ;;
    quad9-doh | quad9-doh-direct | quad9) printf '%s\n' "${_profile:-quad9-doh}" ;;
    *) printf '%s\n' "default" ;;
    esac
    unset _profile
}

magicnet_dns_bootstrap() {
    _bootstrap="${MAGICNET_BOOTSTRAP_DNS:-}"
    if [ -z "$_bootstrap" ]; then
        _bootstrap="$(magicnet_conf_value "$(magicnet_dns_conf)" MAGICNET_BOOTSTRAP_DNS 2>/dev/null || true)"
    fi
    case "${_bootstrap:-aliyun}" in
    system | android | android-system) printf '%s\n' system ;;
    aliyun | alidns | ali) printf '%s\n' aliyun ;;
    baidu | baidudns) printf '%s\n' baidu ;;
    tencent | dnspod | doh.pub) printf '%s\n' tencent ;;
    *) printf '%s\n' aliyun ;;
    esac
    unset _bootstrap
}

magicnet_dns_normalize_server_addresses() {
    awk '
      {
        for (field = 1; field <= NF; field++) {
          value = $field
          while (substr(value, 1, 1) == "[" || substr(value, 1, 1) == "/") {
            value = substr(value, 2)
          }
          while (substr(value, length(value), 1) == "]" || substr(value, length(value), 1) == ",") {
            value = substr(value, 1, length(value) - 1)
          }
          sub(/%.*/, "", value)
          if (value == "" || value == "0.0.0.0" || value == "::" || value == "::1" || value ~ /^127\./) continue
          if (value ~ /^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$/ || value ~ /^[0-9A-Fa-f:]+$/) {
            if (!seen[value]++) print value
          }
        }
      }
    '
}

magicnet_dns_android_system_servers() {
    if [ -n "${MAGICNET_ANDROID_SYSTEM_DNS_SERVERS:-}" ]; then
        printf '%s\n' "$MAGICNET_ANDROID_SYSTEM_DNS_SERVERS" |
            tr ',;' '  ' | magicnet_dns_normalize_server_addresses
        return 0
    fi

    if command -v dumpsys >/dev/null 2>&1; then
        dumpsys connectivity 2>/dev/null | awk '
          {
            line = $0
            while (match(line, /DnsAddresses: \[[^]]*\]/)) {
              values = substr(line, RSTART + 15, RLENGTH - 16)
              gsub(/,/, " ", values)
              print values
              line = substr(line, RSTART + RLENGTH)
            }
          }
        ' | magicnet_dns_normalize_server_addresses
    fi

    if command -v getprop >/dev/null 2>&1; then
        for _system_dns_property in net.dns1 net.dns2 net.dns3 net.dns4; do
            getprop "$_system_dns_property" 2>/dev/null || true
        done | magicnet_dns_normalize_server_addresses
    fi
    unset _system_dns_property
}

magicnet_dns_bootstrap_server() {
    _bootstrap="$(magicnet_dns_bootstrap)"
    if [ "$_bootstrap" = system ]; then
        _bootstrap_server="$(magicnet_dns_android_system_servers | sed -n '1p')"
        if [ -z "$_bootstrap_server" ]; then
            magicnet_warn "Android system DNS is unavailable; bootstrap DNS apply rejected"
            unset _bootstrap _bootstrap_server
            return 1
        fi
        printf '%s\n' "$_bootstrap_server"
        unset _bootstrap _bootstrap_server
        return 0
    fi

    _bootstrap_ipv6_mode="${MAGICNET_IPV6_MODE:-}"
    [ -n "$_bootstrap_ipv6_mode" ] ||
        _bootstrap_ipv6_mode="$(magicnet_network_policy_value MAGICNET_IPV6_MODE 2>/dev/null || true)"
    _bootstrap_ipv6_available=0
    if command -v ip >/dev/null 2>&1 &&
        ip -6 route show default 2>/dev/null | grep -q '^default'; then
        _bootstrap_ipv6_available=1
    fi
    _bootstrap_ipv6_first=0
    [ "$_bootstrap_ipv6_mode" = "prefer_ipv6" ] && _bootstrap_ipv6_first=1

    case "$_bootstrap" in
    baidu)
        _bootstrap_candidates="4:180.76.76.76"
        _bootstrap_fallback="180.76.76.76"
        ;;
    tencent)
        _bootstrap_candidates="4:1.12.12.12 4:120.53.53.53"
        _bootstrap_fallback="1.12.12.12"
        ;;
    *)
        if [ "$_bootstrap_ipv6_first" -eq 1 ] && [ "$_bootstrap_ipv6_available" -eq 1 ]; then
            _bootstrap_candidates="6:2400:3200::1 6:2400:3200:baba::1 4:223.6.6.6 4:223.5.5.5"
            _bootstrap_fallback="2400:3200::1"
        else
            _bootstrap_candidates="4:223.6.6.6 4:223.5.5.5"
            _bootstrap_fallback="223.6.6.6"
            [ "$_bootstrap_ipv6_mode" = "ipv4_only" ] ||
                [ "$_bootstrap_ipv6_available" -eq 0 ] ||
                _bootstrap_candidates="$_bootstrap_candidates 6:2400:3200::1 6:2400:3200:baba::1"
        fi
        ;;
    esac

    # Config materialization must not perform synchronous Internet probes.
    # A stopped TUN can make each curl attempt consume its full timeout and a
    # manual start used to probe as many as four addresses.  Select the first
    # policy-compatible static bootstrap address; sing-box owns reachability
    # and retry behavior after the core is running.
    _bootstrap_server=${_bootstrap_candidates%% *}
    _bootstrap_server=${_bootstrap_server#*:}
    [ -n "$_bootstrap_server" ] || _bootstrap_server="$_bootstrap_fallback"
    printf '%s\n' "$_bootstrap_server"
    unset _bootstrap _bootstrap_ipv6_mode _bootstrap_ipv6_available _bootstrap_ipv6_first
    unset _bootstrap_candidates _bootstrap_server _bootstrap_fallback
}

magicnet_dns_apply_singbox() {
    _profile="$(magicnet_dns_profile)"
    _bootstrap="$(magicnet_dns_bootstrap)"
    _bootstrap_server="$(magicnet_dns_bootstrap_server)" || {
        unset _profile _bootstrap _bootstrap_server
        return 1
    }
    case "$_bootstrap" in
    system | baidu)
        _bootstrap_type=udp
        _bootstrap_sni=
        ;;
    tencent)
        _bootstrap_type=https
        _bootstrap_sni=doh.pub
        ;;
    *)
        _bootstrap_type=https
        _bootstrap_sni=dns.alidns.com
        ;;
    esac
    _config="$(magicnet_singbox_config_file)"
    [ -f "$_config" ] || {
        unset _profile _bootstrap _bootstrap_server _bootstrap_type _bootstrap_sni _config
        return 0
    }
    _jq="$(magicnet_require_jq "packaged jq is unavailable; DNS profile apply rejected")" || {
        unset _profile _bootstrap _bootstrap_server _bootstrap_type _bootstrap_sni _config
        return 1
    }
    _via_proxy="${MAGICNET_DNS_VIA_PROXY:-}"
    [ -n "$_via_proxy" ] || _via_proxy="$(magicnet_conf_value "$(magicnet_dns_conf)" MAGICNET_DNS_VIA_PROXY 2>/dev/null || true)"
    case "$_via_proxy" in
    0 | false | no | off) _via_proxy=0 ;;
    *) _via_proxy=1 ;;
    esac
    _tmp="${_config}.magicnet-dns.new"
    magicnet_jq_install_config "$_config" "$_tmp" "$_jq" --arg profile "$_profile" --arg bootstrap "$_bootstrap" \
        --arg bootstrap_server "$_bootstrap_server" --arg bootstrap_type "$_bootstrap_type" --arg bootstrap_sni "$_bootstrap_sni" \
        --argjson dns_capture_singbox_mark "$(magicnet_dns_capture_singbox_mark)" --argjson via_proxy "$_via_proxy" -e '
      # Build a DNS server definition for a given provider and transport.
      # When via_proxy is true, the server uses detour:"proxy" so queries
      # are routed through the sing-box proxy outbound. When false, the
      # server is contacted directly (or marked for kernel-bypass exemption).
      def make_udp($tag; $server; $via_proxy; $mark):
        if $via_proxy then
          {"type":"udp","tag":$tag,"server":$server,"detour":"proxy"}
        else
          {"type":"udp","tag":$tag,"server":$server,"routing_mark":$mark}
        end;
      def make_tls($tag; $server; $sni; $via_proxy; $mark):
        if $via_proxy then
          {"type":"tls","tag":$tag,"server":$server,"server_port":853,"detour":"proxy","tls":{"server_name":$sni}}
        else
          {"type":"tls","tag":$tag,"server":$server,"server_port":853,"routing_mark":$mark,"tls":{"server_name":$sni}}
        end;
      def make_https($tag; $server; $path; $sni; $via_proxy; $mark):
        if $via_proxy then
          {"type":"https","tag":$tag,"server":$server,"server_port":443,"detour":"proxy","path":$path,"tls":{"server_name":$sni}}
        else
          {"type":"https","tag":$tag,"server":$server,"server_port":443,"routing_mark":$mark,"path":$path,"tls":{"server_name":$sni}}
        end;
      # Provider configuration table: maps profile -> provider details
      def provider_for($profile):
        if $profile == "default" then {tag_prefix:"bootstrap-local-dns",via_proxy:false}
        elif ($profile | startswith("cloudflare")) then
          {tag_prefix:"cloudflare",primary:"1.1.1.1",secondary:"1.0.0.1",sni:"cloudflare-dns.com",via_proxy:($via_proxy == 1 and ($profile | endswith("-direct") | not))}
        elif ($profile | startswith("google")) then
          {tag_prefix:"google",primary:"8.8.8.8",secondary:"8.8.4.4",sni:"dns.google",via_proxy:($via_proxy == 1 and ($profile | endswith("-direct") | not))}
        elif ($profile | startswith("adguard")) then
          {tag_prefix:"adguard",primary:"94.140.14.14",secondary:"",sni:"dns.adguard-dns.com",via_proxy:($via_proxy == 1 and ($profile | endswith("-direct") | not))}
        elif ($profile | startswith("quad9")) then
          {tag_prefix:"quad9",primary:"9.9.9.9",secondary:"149.112.112.112",sni:"dns.quad9.net",via_proxy:($via_proxy == 1 and ($profile | endswith("-direct") | not))}
        else {tag_prefix:"bootstrap-local-dns",via_proxy:false} end;
      # These tags are template-owned DNS policy aliases. Rewrite their rule
      # references to the selected profile so a profile change is global for
      # application DNS instead of only changing dns.final.
      def managed_server_tags:
        ["bootstrap-local-dns","default-remote-dns","doh-cloudflare","doh-google",
         "cloudflare-profile-dns","cloudflare-backup-dns",
         "google-profile-dns","google-backup-dns","adguard-profile-dns","adguard-backup-dns",
         "quad9-profile-dns","quad9-backup-dns"];
      def rewrite_rule_tags:
        ["default-remote-dns","doh-cloudflare","doh-google",
         "cloudflare-profile-dns","cloudflare-backup-dns","google-profile-dns","google-backup-dns",
         "adguard-profile-dns","adguard-backup-dns","quad9-profile-dns","quad9-backup-dns"];
      def profile_dns_tag:
        if $profile == "default" then "bootstrap-local-dns"
        else (provider_for($profile)).tag_prefix + "-profile-dns" end;
      # Resolve which transport to use based on profile suffix
      def transport_for($profile):
        if ($profile | endswith("-udp")) or ($profile | endswith("-udp-direct")) then "udp"
        elif ($profile | endswith("-dot")) or ($profile | endswith("-dot-direct")) then "tls"
        else "https"
        end;
      def server_tags_for($provider):
        [$provider.tag_prefix + "-profile-dns", $provider.tag_prefix + "-backup-dns"];
      def build_server($transport; $tag; $server; $provider; $mark):
        if $transport == "udp" then make_udp($tag; $server; $provider.via_proxy; $mark)
        elif $transport == "tls" then make_tls($tag; $server; $provider.sni; $provider.via_proxy; $mark)
        else make_https($tag; $server; "/dns-query"; $provider.sni; $provider.via_proxy; $mark) end;
      def managed_tags:
        managed_server_tags;
      def default_bootstrap:
        if $bootstrap_type == "udp" then
          make_udp("bootstrap-local-dns"; $bootstrap_server; false; $dns_capture_singbox_mark)
        else
          {"type":"https","tag":"bootstrap-local-dns","server":$bootstrap_server,"server_port":443,"routing_mark":$dns_capture_singbox_mark,"path":"/dns-query","headers":{"Host":$bootstrap_sni},"tls":{"server_name":$bootstrap_sni}}
        end;
      .dns.servers = (
        (.dns.servers // [])
        | map(select((.tag // "") as $tag | managed_tags | index($tag) | not))
        | (if $profile == "default" then [default_bootstrap]
           else
             (provider_for($profile) | . as $provider |
              (transport_for($profile) | . as $transport |
               [default_bootstrap,
                build_server($transport; ($provider.tag_prefix + "-profile-dns"); $provider.primary; $provider; $dns_capture_singbox_mark),
                (if $provider.secondary then build_server($transport; ($provider.tag_prefix + "-backup-dns"); $provider.secondary; $provider; $dns_capture_singbox_mark) else empty end)
               ]))
           end) + .
      )
      # Direct UDP DNS servers are contacted by sing-box itself. Mark those
      # sockets so the kernel DNS redirect can exempt them without exempting
      # every UID-0 Android resolver query.
      | .dns.servers |= map(
          if (.type == "udp" and (.detour // "") == "") then
            .routing_mark = $dns_capture_singbox_mark
          else .
          end
        )
      | .dns.rules = ((.dns.rules // []) | map(
          if ((.server // "") as $server | rewrite_rule_tags | index($server)) != null
          then .server = profile_dns_tag
          else .
          end
        ))
      # route.default_domain_resolver remains bootstrap-local-dns for proxy
      # node hostnames; using a proxy-detoured profile there would recurse.
      | .dns.final = profile_dns_tag
      # sing-box 1.14 adds per-query timeout, optimistic DNS caching and DNS
      # cache persistence. Apply conservative defaults only when the user has
      # not made an explicit choice. A disabled cache remains disabled.
      | if (.dns | has("timeout") | not) then .dns.timeout = "8s" else . end
      | if ((.dns.disable_cache // false) == false) then
          (if (.dns | has("cache_capacity") | not) then .dns.cache_capacity = 4096 else . end)
          | (if ((.dns.disable_expire // false) == false and (.dns | has("optimistic") | not)) then
               .dns.optimistic = {"enabled":true,"timeout":"30m"}
             else . end)
          | (if ((.experimental // null) | type) != "object" then .experimental = {} else . end)
          | (if ((.experimental.cache_file // null) | type) != "object" then .experimental.cache_file = {} else . end)
          | (if (.experimental.cache_file | has("enabled") | not) then .experimental.cache_file.enabled = true else . end)
          | (if ((.experimental.cache_file.enabled // false) == true and (.experimental.cache_file | has("store_dns") | not)) then
               .experimental.cache_file.store_dns = true
             else . end)
        else . end
    ' "$_config"
    _rc=$?
    unset _profile _bootstrap _bootstrap_server _bootstrap_type _bootstrap_sni _config _jq _tmp _via_proxy
    return "$_rc"
}

magicnet_dns_apply_unlocked() {
    magicnet_dns_apply_singbox
}

magicnet_dns_apply() {
    magicnet_with_config_lock magicnet_dns_apply_unlocked
}
