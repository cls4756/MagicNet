import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { APPLICATION_DNS_TARGET, BOOTSTRAP_DNS_TARGET, buildDnsRulesPayload, copyDnsRuleRows, parseDnsRulesSnapshot, supportsDnsInspection, validateDnsConditions } from "./src/components/pages/dnsRulesPlan.ts";
import { buildDnsProfilePlan } from "./src/components/pages/dnsProfilePlan.ts";

const envelope = (command, data) => JSON.stringify({ schema: 1, ok: true, command, data });
const data = {
  revision: "a".repeat(64), source: "configuration", truncated: false, runtime_state: "unknown",
  rules: [
    { id: 1, conditions: { clash_mode: "Global" }, target: APPLICATION_DNS_TARGET, editable: false, protected_reason: "mode" },
    { id: 2, conditions: { domain_suffix: ["cn"] }, target: BOOTSTRAP_DNS_TARGET, editable: true, protected_reason: null },
    { id: 3, conditions: { rule_set: ["foreign"] }, target: APPLICATION_DNS_TARGET, editable: true, protected_reason: null },
  ],
  resolvers: [
    { tag: APPLICATION_DNS_TARGET, kind: "application", available: true },
    { tag: BOOTSTRAP_DNS_TARGET, kind: "bootstrap", available: true },
  ],
  rule_sets: ["foreign"], final_server: "cloudflare-profile-dns", node_resolver: BOOTSTRAP_DNS_TARGET, application_server: "cloudflare-profile-dns",
};
const snapshot = () => parseDnsRulesSnapshot(envelope("dns.inspect", data));

test("DNS rules negotiate explicit capabilities and reject ambiguous/malformed snapshots", () => {
  assert.equal(supportsDnsInspection(envelope("machine.capabilities", { commands: ["dns.inspect"] })), true);
  assert.equal(supportsDnsInspection(envelope("machine.capabilities", { commands: ["dns.status"] })), false);
  assert.ok(snapshot());
  for (const text of ["bad", envelope("dns.status", data), envelope("dns.inspect", data) + "\n" + envelope("dns.inspect", data),
    envelope("dns.inspect", { ...data, revision: "bad" }), envelope("dns.inspect", { ...data, rules: [data.rules[1]] }),
    envelope("dns.inspect", { ...data, truncated: "false" }), envelope("dns.inspect", { ...data, source: "runtime" }),
    envelope("dns.inspect", { ...data, resolvers: [...data.resolvers, data.resolvers[0]] })]) {
    assert.equal(parseDnsRulesSnapshot(text), null);
  }
});

test("DNS payload retains protected rules by reference and preserves independent Bootstrap targets", () => {
  const current = snapshot();
  const rows = copyDnsRuleRows(current.rules);
  rows[2].conditions = { domain: ["edited.invalid"] };
  [rows[1], rows[2]] = [rows[2], rows[1]];
  rows.push({ id: null, key: "new", conditions: { rule_set: ["foreign"] }, target: APPLICATION_DNS_TARGET, editable: true, protectedReason: null });
  const payload = buildDnsRulesPayload(current, rows);
  assert.deepEqual(payload.rules[0], { id: 1 });
  assert.equal(payload.rules[1].target, APPLICATION_DNS_TARGET);
  assert.equal(payload.rules[2].target, BOOTSTRAP_DNS_TARGET);
  assert.equal(payload.rules[3].id, null);
  assert.deepEqual(current.rules[2].conditions, { rule_set: ["foreign"] });
});

test("protected rules cannot be changed removed or moved; stale/truncated inputs stay blocked", () => {
  const current = snapshot();
  for (const mutate of [
    (rows) => { rows[0].target = BOOTSTRAP_DNS_TARGET; },
    (rows) => { rows[0].conditions = {}; },
    (rows) => { rows.shift(); },
    (rows) => { [rows[0], rows[1]] = [rows[1], rows[0]]; },
    (rows) => { rows[2].id = 2; },
    (rows) => { rows[2].target = "missing"; },
  ]) {
    const rows = copyDnsRuleRows(current.rules);
    mutate(rows);
    assert.throws(() => buildDnsRulesPayload(current, rows));
  }
  assert.throws(() => buildDnsRulesPayload({ ...current, truncated: true }, current.rules));
});

test("condition validation bounds values and rejects unknown keys, types and rule-set references", () => {
  validateDnsConditions({ domain_suffix: ["cn"], query_type: ["A", 28], port: [53], ip_is_private: false }, ["foreign"]);
  for (const conditions of [null, [], {}, { server: "wrong" }, { rule_set: ["missing"] }, { domain: [] },
    { domain: Array(129).fill("test.invalid") }, { domain: "x".repeat(257) }, { domain: "bad\n.invalid" },
    { domain: "bad\u0085.invalid" }, { port: 0 }, { port: 65536 }, { port: 1.5 }, { invert: "false" }, { domain: 42 }]) {
    assert.throws(() => validateDnsConditions(conditions, ["foreign"]));
  }
});

test("DNS page owns the editor and uses private transport with unconditional payload cleanup", () => {
  const page = readFileSync(new URL("./src/components/pages/SettingsDnsPage.vue", import.meta.url), "utf8");
  const panel = readFileSync(new URL("./src/components/pages/DnsRulesPanel.vue", import.meta.url), "utf8");
  assert.match(page, /<DnsRulesPanel\s*\/>/);
  assert.match(panel, /runPrivateCli\("--json dns inspect"/);
  assert.match(panel, /stagePrivatePayload\("tmp"/);
  assert.match(panel, /finally\s*\{\s*const cleaned = await removePrivatePayload/);
  assert.match(panel, /disposed \|\| sequence !== readSequence/);
  assert.match(panel, /<label[^>]*>\{\{ t\("匹配条件（JSON）"\)/);
  assert.match(panel, /t\('上移第 \{order\} 条 DNS 规则'/);
  assert.doesNotMatch(panel, /runCli\("--json dns inspect"|localStorage|sessionStorage/);
});

test("default keeps its persisted value and clearly follows Bootstrap rather than replacing it", () => {
  const plan = buildDnsProfilePlan("cloudflare-doh", "default");
  assert.equal(plan.profile, "default");
  assert.equal(plan.primary, BOOTSTRAP_DNS_TARGET);
  assert.match(plan.items[0].value, /跟随 Bootstrap DNS/);
  const remote = buildDnsProfilePlan("default", "cloudflare-doh");
  assert.match(remote.warnings.join("\n"), /显式指向 Bootstrap DNS 的规则保持不变/);
});
