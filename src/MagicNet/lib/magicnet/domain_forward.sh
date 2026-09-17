# shellcheck shell=ash
#
# Domain forwarding for the transparent inbounds.
#
# Apps behind magicnet0 / the ebpf inbound reach sing-box with packets that only
# carry an IP address. Sniffing recovers the TLS SNI or HTTP Host, but the
# outbound still dials the IP, so a proxy node cannot apply its own domain based
# routing. Enabling this hands the sniffed domain to the outbound instead.
#
# The sing-box option behind it is a fork change: see sing-box-patches/. A core
# without that change rejects the key outright, so the feature is only written
# into the running config when the installed binary advertises the capability.
# Everything else reports the gap as configured-but-ineffective instead of
# silently claiming the toggle works.

magicnet_domain_forward_conf() {
    printf '%s\n' "${MODDIR}/.config/magicnet/domain-forward.conf"
}

magicnet_domain_forward_enabled() {
    _df_conf=$(magicnet_domain_forward_conf)
    _df_value=""
    if [ -f "$_df_conf" ]; then
        _df_value=$(sed -n 's/^[[:space:]]*MAGICNET_DOMAIN_FORWARD[[:space:]]*=[[:space:]]*//p' "$_df_conf" |
            tail -n 1 | tr -d '\r')
    fi
    case "$(printf '%s' "$_df_value" | tr '[:upper:]' '[:lower:]')" in
    0 | false | off | no | disabled)
        _df_enabled=0
        ;;
    *)
        _df_enabled=1
        ;;
    esac
    unset _df_conf _df_value
    if [ "$_df_enabled" -eq 1 ]; then
        unset _df_enabled
        return 0
    fi
    unset _df_enabled
    return 1
}

# Capability probe. The option only reaches the binary through the patch in
# sing-box-patches/, and struct tags are part of the compiled type metadata, so
# a scan of the installed core answers "does this build understand the key"
# without starting sing-box.
#
# The `json:"` prefix is what keeps the scan honest. Upstream still declares
# `sniff_override_destination` on the inbound options, so matching the bare field
# name reports every stock core as capable and the rewrite then publishes a key
# the core refuses to decode. The decoder itself is the final authority.
magicnet_domain_forward_supported() {
    _df_bin="${MODDIR}/bin/sing-box"
    [ -f "$_df_bin" ] || {
        unset _df_bin
        return 1
    }
    LC_ALL=C grep -qa 'json:"override_destination' "$_df_bin" 2>/dev/null || {
        unset _df_bin
        return 1
    }
    unset _df_bin
    magicnet_singbox_decodes_domain_forward
}

# Authoritative capability check: let the installed core decode a probe document
# that uses the fork option. A core that exits on an unknown key takes the whole
# dataplane down with it, so the rewrite never publishes a document the decoder
# rejects. The probe is staged next to the runtime config, where the other
# transient dot files already live, and removed again immediately.
magicnet_singbox_decodes_domain_forward() {
    _df_core="${MODDIR}/bin/sing-box"
    [ -x "$_df_core" ] || {
        unset _df_core
        return 1
    }
    _df_probe="$(mktemp "${MODDIR}/.config/sing-box/.domain-forward-probe.XXXXXX" 2>/dev/null)" || {
        # Without a place to stage the probe the capability stays unknown, and an
        # unknown capability must never reach the runtime configuration.
        unset _df_core
        return 1
    }
    printf '%s\n' '{"route":{"rules":[{"inbound":["tun-in"],"network":["tcp"],"action":"sniff","override_destination":true}]}}' >"$_df_probe" 2>/dev/null
    "$_df_core" check -c "$_df_probe" >/dev/null 2>&1
    _df_rc=$?
    rm -f "$_df_probe" 2>/dev/null
    unset _df_core _df_probe
    return "$_df_rc"
}

magicnet_domain_forward_effective() {
    magicnet_domain_forward_enabled || return 1
    magicnet_domain_forward_supported || return 1
    return 0
}

# Rewrite the sniff rules of one sing-box config so the sniffed domain replaces
# the packet destination for TCP. UDP keeps the current IP based path: the
# override is scoped to a dedicated `network: tcp` rule. Because only this
# function ever writes `override_destination`, that key doubles as the marker
# that makes the rewrite idempotent.
magicnet_singbox_apply_domain_forward() {
    _df_config="$1"
    if [ ! -f "$_df_config" ]; then
        unset _df_config
        return 0
    fi
    _df_jq="$(command -v jq 2>/dev/null || true)"
    if [ -z "$_df_jq" ]; then
        # Domain forwarding is an enhancement, not a precondition. The recovery
        # paths deliberately tolerate a missing jq, so an unavailable rewrite
        # must not turn a working runtime apply into a failed one. The status
        # command reports the missing rule as `pending` instead.
        command -v magicnet_warn >/dev/null 2>&1 &&
            magicnet_warn "jq not found; domain forwarding stays unwritten"
        unset _df_config _df_jq
        return 0
    fi
    _df_want=false
    magicnet_domain_forward_effective && _df_want=true
    _df_tmp="${_df_config}.domain-forward.new"
    if (
        umask 077
        "$_df_jq" --argjson want "$_df_want" '
        def is_sniff: (.action // "") == "sniff";
        # The override key is only ever written here, so its presence is the
        # unique fingerprint of a previously generated rule. Matching on the key
        # instead of on `network: tcp` keeps a hand-written tcp sniff rule safe.
        def is_domain_forward_rule: is_sniff and has("override_destination");
        def domain_forward_rule:
          {"inbound":["tun-in"],"network":["tcp"],"action":"sniff","override_destination":true};
        def plain_sniff_rule:
          {"inbound":["mixed-in","tun-in"],"action":"sniff"};
        (.route.rules // []) as $rules
        | ($rules
            | map(select(is_domain_forward_rule | not))
            | map(if is_sniff then del(.override_destination) else . end)) as $clean
        | ([$clean | to_entries[] | select(.value | is_sniff) | .key] | first) as $anchor
        # Leave the document semantically untouched when nothing has to change.
        # The runtime fingerprint compares canonical JSON, so a needless rewrite
        # is harmless there, but a no-op keeps the intent obvious.
        | if $want or $clean != $rules then
            .route.rules =
              (if $want then
                (if $anchor == null
                 then $clean + [plain_sniff_rule, domain_forward_rule]
                 else $clean[0:$anchor] + [domain_forward_rule] + $clean[$anchor:]
                 end)
               else $clean
               end)
          else .
          end
        ' "$_df_config" >"$_df_tmp"
    ); then
        :
    else
        rm -f "$_df_tmp" 2>/dev/null || true
        unset _df_config _df_jq _df_want _df_tmp
        return 1
    fi
    if ! chmod 600 "$_df_tmp" || ! mv -f "$_df_tmp" "$_df_config" || ! chmod 600 "$_df_config"; then
        rm -f "$_df_tmp" 2>/dev/null || true
        unset _df_config _df_jq _df_want _df_tmp
        return 1
    fi
    unset _df_config _df_jq _df_want _df_tmp
}