# Maintained routing classifiers

Business domains and service IPs are maintained upstream, rather than duplicated
in the MagicNet template:

- [MetaCubeX/meta-rules-dat](https://github.com/MetaCubeX/meta-rules-dat/tree/sing/geo/geosite): named AI services, Apple/iCloud, Bing/Microsoft, developers, gaming downloads, games, media/entertainment, social/communication, Telegram, connectivity, encrypted DNS and IP diagnostics.
- [lyc8503/sing-box-rules](https://github.com/lyc8503/sing-box-rules): existing CN, advertising and Telegram GeoIP classifiers.
- [KaringX/karing-ruleset](https://github.com/KaringX/karing-ruleset): existing WeChat and China domain/IP classifiers.
- [HaGeZi via razaxq](https://github.com/razaxq/dns-blocklists-sing-box): light advertising/tracking classifier. Copyright/anti-piracy lists are not part of the default advertising policy.
- [SukkaW/ruleset.skk.moe](https://gitlab.com/SukkaW/ruleset.skk.moe/-/blob/master/sing-box/ip/ai.json): ChatGPT Voice IPs, generated from OpenAI's published feed.
- [LIghtJUNction/MagicNetRules](https://github.com/LIghtJUNction/MagicNetRules): unified, deduplicated and optimized sing-box rule sets packaged via git submodule `rules`.

Build hooks resolve immutable upstream revisions, synchronize from the local `rules` submodule,
and package local rule sets. New upstream data arrives with a new build; ordinary
subscription refresh does not update these classifiers. Startup works without
fetching rules from their upstream hosts. Build state records the resolved revisions.

## Policy that remains local

MagicNet retains mode selection, per-app choices, LAN and reserved addresses,
the explicit domestic connectivity probe exception, and the mapping from a
classifier to an outbound. These are device/product policy, not business lists.
No generic `analytics`, `tracker`, `github`, or `captive` substring routing remains.

Rule ordering gives explicit modes precedence, protects LAN traffic, applies
native app hints, and uses maintained advertising classifiers before general
business classification. Specific AI services precede general AI; iCloud precedes
Apple; Bing and development/game services precede Microsoft. Gaming download
endpoints precede gaming login/community traffic. Telegram precedes general
communication. Known business domains precede generic country IP ownership.

Domestic business rules default directly to `direct`; LAN and advertisement
safeguards remain module-managed. Foreign business rules, including AI, default
to `proxy-auto`. The public default groups are `proxy`, `proxy-auto`, and
`final`; without nodes the automatic path is blocked, not direct. No per-service
AI geography filtering or automatic failover group is created by default.
Users can target individual nodes or create reusable groups where needed.
DNS domain classifiers use the same ordering and matching conditions as route
classifiers. Direct service classes resolve through local encrypted DNS; proxy
classes through the remote resolvers. IP/protocol-only routes are not copied into
DNS rules. Manual DNS-profile overrides retain their existing behavior.

## Validation

`python3 scripts/test-maintained-routing.py` checks references, selector defaults,
ordering and DNS/route classifier parity without requiring downloads.
`--assets` additionally calls the sing-box matcher on actual bundled upstream
files for TCP/UDP first-match cases, DNS ownership, explicit modes, LAN,
advertising, voice ports, foreign-service/CN-IP overlap and keyword false positives.
Legacy embedded-list tests remain available for legacy templates.

## User routing policy

The Proxy page manages ordered business rules, their direct outbound targets,
optional reusable user groups, and the fallback. Conditions support domain
matches, rule sets, IPs, ports, protocols, package names, and bounded logical
AND/OR combinations. System/feature-owned rules remain protected. Adjacent pure
classifier lists with identical targets can be merged; merging does not cross
an exception or a protected rule and does not turn OR conditions into AND.

`proxy` is manual selection, initially choosing `proxy-auto`; `final` chooses
`proxy-auto` unless the user configures another fallback. A group such as
`ai-grok` can be created by the user but is not a required service abstraction.
Saved policy is materialized after module feature producers and before core
startup or subscription candidate validation, so those producers cannot restore
deleted business rules or discard user groups. Legacy subscription sanitizers
may still construct old selectors as migration inputs; they are removed from
the effective configuration by the routing materializer.

Explicit persisted legacy service selections migrate to direct rule targets,
or to optional `user-<legacy-tag>` groups when a selected group must be retained.
Migration intent is published with the active configuration, not while merely
rendering a subscription candidate. Standalone configurations are not silently
simplified unless the user saves a routing policy. The legacy domain CLI forwards
to user policy once it exists; old domain-list writers no longer overwrite it.

Without explicit user DNS-rule intent, domain classifiers are projected from
the ordered business routing policy: direct rules use local encrypted DNS and
proxy rules use the application profile; IP/port/protocol/package-only routes
are not projected. Explicit `.config/magicnet/dns-rules.json` takes precedence
and remains owned by the DNS editor. Mode, bootstrap, LAN, and feature DNS rules
are preserved.

MetaCubeX supplies the named domain sets, including the newly referenced
`twitter.srs` and `whatsapp.srs`. Gemini and YouTube precede Google; named
services precede broad developer, media, social and domestic domain rules.
Google Play/services and the X Android app use the corresponding maintained
classifiers, targeting `proxy-auto` by default without mandatory service selectors.

Run `cargo test -p magicnet-cli routing_policy`,
`cargo test -p magicnet-cli --test routing_policy`, and `cd webui && npm run check`
for policy validation, persistence, rollback, UI contract, and translation tests.
`webui/e2e/routing-policy.spec.mjs` covers rule CRUD, ordering, group reuse and
renaming, private payload cleanup, and old-module capability refusal.
