import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const dnsCard = readFileSync(new URL("./src/components/pages/DnsToolsCard.vue", import.meta.url), "utf8");
const networkCard = readFileSync(new URL("./src/components/pages/NetworkPolicyCard.vue", import.meta.url), "utf8");

test("DNS interception is configured from the DNS card", () => {
  assert.match(dnsCard, /t\("DNS 劫持"\)/);
  assert.match(dnsCard, /@change="setDnsInterception/);
  assert.doesNotMatch(networkCard, /t\("DNS 劫持"\)/);
});

test("DNS profile and bootstrap selectors require interception", () => {
  assert.match(dnsCard, /dnsInterception\.value === "on"/);
  assert.match(dnsCard, /!pendingDnsAction\.value\?\.key\.startsWith\("dns-interception-"\)/);
  assert.equal((dnsCard.match(/:disabled="!dnsSettingsEnabled"/g) ?? []).length, 2);
});

test("network policy updates preserve DNS interception", () => {
  assert.match(networkCard, /status\.configured\.dns_interception/);
  assert.match(networkCard, /network set \$\{ipv6Mode\.value\} \$\{mtu\.value\} \$\{udpTimeout\.value\} \$\{dnsInterception\.value\}/);
  assert.match(networkCard, /:disabled="!networkPolicyLoaded"/);
});
