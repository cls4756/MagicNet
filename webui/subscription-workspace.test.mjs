import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const read = (relativePath) =>
  readFileSync(new URL(relativePath, import.meta.url), "utf8");

const subscriptions = read("./src/components/pages/SubscriptionsPage.vue");
const proxyGroups = read("./src/components/pages/ProxyGroupsPanel.vue");
const diagnostics = read("./src/components/pages/DiagnosticsPage.vue");

assert.match(subscriptions, /<SubscriptionSourceList/);
assert.doesNotMatch(subscriptions, /<ProxyGroupsPanel/);
assert.match(subscriptions, /<SubscriptionLifecycleStrip :configured="configured" \/>/);
assert.match(subscriptions, /更新记录与高级设置/);
assert.match(proxyGroups, /const allFilteredGroups = computed/);
assert.match(proxyGroups, /const visibleGroups = computed/);
assert.match(proxyGroups, /groupKindLabel/);
assert.doesNotMatch(proxyGroups, /proxy-view-tabs/);
assert.match(proxyGroups, /按策略组查看和切换当前代理节点/);
assert.match(proxyGroups, /proxy-group-grid/);
assert.match(proxyGroups, /expandedGroups\.has\(group\.name\)/);
assert.match(proxyGroups, /lastSuccessEpoch/);
assert.doesNotMatch(diagnostics, /<ProxyGroupsPanel/);
assert.doesNotMatch(diagnostics, /<NodeDelayPanel/);

console.log("subscription workspace contract tests passed");
