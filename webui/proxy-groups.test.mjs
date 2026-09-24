import assert from "node:assert/strict";
import test from "node:test";
import { parseProxyGroupsSnapshot } from "./src/composables/proxyGroupParsers.ts";
import { buildProxySelectionPlan } from "./src/components/pages/proxySelectionPlan.ts";

test("native /proxies groups expose all members for selection and group testing", () => {
  const snapshot = parseProxyGroupsSnapshot(JSON.stringify({
    proxies: {
      proxy: { type: "Selector", now: "auto", all: ["auto", "US node", "日本"] },
      auto: { type: "URLTest", now: "US node", all: ["US node", "日本"] },
      "US node": { type: "Shadowsocks", history: [] },
      "日本": { type: "Shadowsocks", history: [] },
      direct: { type: "Direct" },
    },
  }));
  assert.deepEqual(snapshot.groups, [
    { name: "proxy", type: "Selector", now: "auto", proxies: ["auto", "US node", "日本"], kind: "selector", selectable: true },
    { name: "auto", type: "URLTest", now: "US node", proxies: ["US node", "日本"], kind: "auto", selectable: false },
  ]);
  assert.deepEqual(snapshot.nodes, [
    { name: "US node", type: "Shadowsocks" },
    { name: "日本", type: "Shadowsocks" },
  ]);
  const plan = buildProxySelectionPlan(snapshot.groups[0], "US node", [{
    node: "US node", summary: "40ms", delayMillis: 40, quality: "fast",
  }]);
  assert.equal(plan.status, "ready");
  assert.deepEqual(plan.warnings, []);
});

test("provider and legacy group responses retain proxies member support", () => {
  const group = { name: "provider", type: "provider", now: "", proxies: ["one", "two"], kind: "provider", selectable: false };
  const source = { proxies: ["one", { name: "two" }, null, 12, {}] };
  assert.deepEqual(
    parseProxyGroupsSnapshot(JSON.stringify({ providers: { provider: source } })).groups,
    [group],
  );
  assert.deepEqual(parseProxyGroupsSnapshot(JSON.stringify({ provider: source })).groups, [group]);
});

test("native all members take precedence, including an explicitly empty list", () => {
  const snapshot = parseProxyGroupsSnapshot(JSON.stringify({
    proxies: {
      proxy: { now: "active", all: ["active"], proxies: ["stale"] },
      empty: { now: "previous", all: [], proxies: ["stale"] },
    },
  }));
  assert.deepEqual(snapshot.groups.map((group) => group.proxies), [["active"], []]);
});

test("invalid snapshot roots are rejected while an empty proxies map is valid", () => {
  for (const source of ["null", "false", "42", '"text"', "[]", "not json"]) {
    assert.equal(parseProxyGroupsSnapshot(source), null, source);
  }
  assert.deepEqual(parseProxyGroupsSnapshot('{"proxies":{}}'), { groups: [], nodes: [] });
});

import { userVisibleProxyGroups } from "./src/composables/proxyGroupParsers.ts";

test("user-facing proxy groups keep primary tabs first and hide internal implementation groups", () => {
  const groups = [
    { name: "ai-proxy", type: "selector", now: "proxy", proxies: ["proxy"], kind: "selector", selectable: true },
    { name: "provider-selector", type: "selector", now: "node-a", proxies: ["node-a"], kind: "provider", selectable: false },
    { name: "final", type: "selector", now: "proxy", proxies: ["proxy"], kind: "selector", selectable: true },
    { name: "proxy-auto", type: "urltest", now: "node-a", proxies: ["node-a"], kind: "auto", selectable: false },
    { name: "proxy", type: "selector", now: "node-a", proxies: ["node-a"], kind: "selector", selectable: true },
  ];
  assert.deepEqual(userVisibleProxyGroups(groups).map((group) => group.name), [
    "proxy", "proxy-auto", "final", "provider-selector",
  ]);
});
