import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { reactive } from "vue";
import {
  buildRoutingPayload, copyRoutingPolicy, parseRoutingPolicySnapshot, renameRoutingGroup,
  routingGroupReferenced, routingTargets, supportsRoutingPolicy, validateRoutingConditions,
} from "./src/components/pages/routingPolicyPlan.ts";

const envelope = (command, data) => JSON.stringify({ schema: 1, ok: true, command, data });
const fixture = () => ({
  schema: 1, revision: "a".repeat(64), source: "user_intent", protected_count: 4,
  rules: [{ conditions: { rule_set: ["grok", "gemini"] }, target: "proxy-auto", available: true }],
  groups: [], final: "proxy-auto", rule_sets: ["grok", "gemini"],
  targets: [{ tag: "proxy", type: "selector" }, { tag: "proxy-auto", type: "urltest" },
    { tag: "final", type: "selector" }, { tag: "node-a", type: "socks" },
    { tag: "direct", type: "direct" }, { tag: "block", type: "block" }],
});
const snapshot = () => parseRoutingPolicySnapshot(envelope("routing.inspect", { policy: fixture() }));

test("routing policy requires explicit capability and private read negotiation", () => {
  const capabilities = { commands: ["routing.inspect"], private_commands: ["routing.inspect"], features: ["routing_policy_management"] };
  assert.equal(supportsRoutingPolicy(envelope("machine.capabilities", capabilities)), true);
  for (const value of [{}, { ...capabilities, features: [] }, { ...capabilities, private_commands: [] }, { ...capabilities, commands: "routing.inspect" }]) {
    assert.equal(supportsRoutingPolicy(envelope("machine.capabilities", value)), false);
  }
});

test("routing policy parser preserves intent, revision, merged classifier lists and safeguards", () => {
  const parsed = snapshot();
  assert.equal(parsed.protectedCount, 4);
  assert.deepEqual(parsed.rules[0].conditions, { rule_set: ["grok", "gemini"] });
  assert.equal(parsed.final, "proxy-auto");
  assert.equal(parseRoutingPolicySnapshot("{}"), null);
  const oversized = fixture();
  oversized.rules = Array(257).fill(oversized.rules[0]);
  assert.equal(parseRoutingPolicySnapshot(envelope("routing.inspect", { policy: oversized })), null);
});

test("CRUD payload selects nodes directly, maintains order and excludes observation fields", () => {
  const parsed = snapshot();
  const policy = copyRoutingPolicy(parsed);
  policy.rules.unshift({ conditions: { domain_suffix: ["grok.com"] }, target: "node-a" });
  policy.rules.splice(1, 1);
  const payload = buildRoutingPayload(parsed, policy);
  assert.equal(payload.revision, "a".repeat(64));
  assert.deepEqual(payload.rules, [{ conditions: { domain_suffix: ["grok.com"] }, target: "node-a" }]);
  assert.equal(JSON.stringify(payload).includes("available"), false);
  assert.equal(JSON.stringify(payload).includes("targets"), false);
});

test("Vue reactive snapshots and editor drafts clone safely without retaining reactive proxies", () => {
  const parsed = reactive(snapshot());
  const policy = reactive(copyRoutingPolicy(parsed));
  policy.rules[0].conditions.rule_set.push("grok");
  assert.deepEqual(parsed.rules[0].conditions.rule_set, ["grok", "gemini"]);
  assert.doesNotThrow(() => buildRoutingPayload(parsed, policy));
});

test("user service groups are optional, reusable, renameable, and reference-safe", () => {
  const parsed = snapshot();
  const policy = copyRoutingPolicy(parsed);
  policy.groups.push({ tag: "ai-grok", type: "selector", members: ["node-a", "proxy-auto"], default: "node-a" });
  policy.rules[0].target = "ai-grok";
  assert.equal(buildRoutingPayload(parsed, policy).groups[0].tag, "ai-grok");
  assert.equal(routingGroupReferenced(policy, "ai-grok"), true);
  renameRoutingGroup(policy, "ai-grok", "my-ai");
  policy.groups[0].tag = "my-ai";
  assert.equal(policy.rules[0].target, "my-ai");
  assert.equal(routingTargets(parsed, policy).some((target) => target.tag === "my-ai"), true);
  policy.groups = [];
  assert.throws(() => buildRoutingPayload(parsed, policy));
});

test("policy validation rejects cycles, unavailable references and non-node automatic members", () => {
  const parsed = snapshot();
  const policy = copyRoutingPolicy(parsed);
  policy.groups = [{ tag: "custom", type: "selector", members: ["final"], default: "final" }];
  policy.final = "custom";
  assert.throws(() => buildRoutingPayload(parsed, policy));
  policy.final = "proxy-auto";
  policy.groups = [{ tag: "custom", type: "urltest", members: ["proxy"], url: "https://example.com/" }];
  assert.throws(() => buildRoutingPayload(parsed, policy));
  policy.groups[0].members = ["node-a"];
  assert.doesNotThrow(() => buildRoutingPayload(parsed, policy));
  policy.rules[0].target = "disappeared-node";
  assert.throws(() => buildRoutingPayload(parsed, policy));
});

test("matching editor supports logical OR without accepting system actions, UID or empty rules", () => {
  assert.doesNotThrow(() => validateRoutingConditions({ type: "logical", mode: "or", rules: [
    { rule_set: ["grok"] }, { domain_suffix: ["example.com"], network: "tcp" },
  ] }, ["grok"]));
  for (const conditions of [{}, { invert: true }, { user_id: [10001] }, { action: "hijack-dns" }, { rule_set: ["missing"] }, { domain_suffix: [] }]) {
    assert.throws(() => validateRoutingConditions(conditions, ["grok"]));
  }
});

test("routing editor uses private staged writes, local drafts and reference-safe group deletion", () => {
  const source = readFileSync(new URL("./src/components/pages/RoutingRulesPanel.vue", import.meta.url), "utf8");
  assert.match(source, /runPrivateCli\("--json routing inspect"/);
  assert.match(source, /stagePrivatePayload\("tmp"/);
  assert.match(source, /routing save-file/);
  assert.match(source, /finally[\s\S]*removePrivatePayload/);
  assert.ok(source.indexOf("await removePrivatePayload") < source.indexOf('emit("applied")'));
  assert.match(source, /routingGroupReferenced/);
  assert.match(source, /onBeforeUnmount/);
  assert.match(source, /function requestDelete[\s\S]*if \(mutationLocked\.value/);
  assert.match(source, /:disabled="mutationLocked"/);
  assert.doesNotMatch(source, /route add-domain|routeTargets|由来源管理/);
});
