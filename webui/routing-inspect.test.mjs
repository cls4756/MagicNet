import assert from "node:assert/strict";
import test from "node:test";
import { parseRoutingInspectSnapshot } from "./src/composables/routingInspect.ts";

test("routing inspector preserves rule order, managed domain metadata, and unknown runtime", () => {
  const parsed = parseRoutingInspectSnapshot(JSON.stringify({
    schema: 1,
    ok: true,
    command: "routing.inspect",
    data: {
      rules: [{ order: 1, conditions: { domain_suffix: ["example.com"] }, outbound: "proxy-rule", source: "magicnet_custom_domain", managed_domains: ["example.com"], managed_target: "proxy" }],
      outbounds: [{ tag: "proxy", type: "selector", source: "magicnet", internal: true, members: ["node-a"], default: "node-a", selected: null, references: [], reference_count: 0 }],
      final_outbound: "final",
      runtime_state: "unknown",
      truncated: false,
    },
  }));
  assert.equal(parsed?.rules[0].order, 1);
  assert.equal(parsed?.rules[0].managedTarget, "proxy");
  assert.equal(parsed?.outbounds[0].selected, null);
  assert.equal(parsed?.finalOutbound, "final");
});

test("routing inspector rejects non-envelope payloads", () => {
  assert.equal(parseRoutingInspectSnapshot("{}"), null);
  assert.equal(parseRoutingInspectSnapshot(JSON.stringify({ schema: 1, ok: true, command: "other", data: {} })), null);
});
