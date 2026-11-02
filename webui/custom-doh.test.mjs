import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import test from "node:test";
import ts from "typescript";
import { buildCustomDohPayload, parseCustomDohSnapshot, supportsCustomDoh, validateCustomDohUrl } from "./src/components/pages/customDohPlan.ts";
import { buildDnsProfilePlan, formatDnsProfilePlanReport } from "./src/components/pages/dnsProfilePlan.ts";
import { parseMachineDns } from "./src/composables/machineStatus.ts";
import { messages } from "./src/i18n/messages.ts";

const envelope = (command, data) => JSON.stringify({ schema: 1, ok: true, command, data });
const data = { configured: true, url: "https://resolver.example:8443/private/dns?key=private-fixture", via_proxy: false, revision: "a".repeat(64) };

test("custom DoH requires explicit private capability negotiation", () => {
  const capabilities = { commands: ["dns.custom.inspect"], private_commands: ["dns.custom.inspect"], features: ["custom_doh"] };
  assert.equal(supportsCustomDoh(envelope("machine.capabilities", capabilities)), true);
  for (const value of [{}, { ...capabilities, features: [] }, { ...capabilities, commands: "dns.custom.inspect" }, { ...capabilities, private_commands: [] }]) assert.equal(supportsCustomDoh(envelope("machine.capabilities", value)), false);
});

test("valid HTTPS custom DoH URLs preserve ports, paths, queries and private addresses", () => {
  for (const url of [data.url, "https://[2001:db8::1]:5443/dns-query", "https://[::ffff:192.0.2.1]/dns-query", "https://192.168.1.2/dns-query", "https://dns.example/dns-query?q=a%20b", "https://dns.example"]) assert.equal(validateCustomDohUrl(url), url);
  for (const url of ["http://dns.example/dns", "https://secret@dns.example/dns", "https://dns.example/#secret", "https://dns.example:0/dns", "https://dns.example:65536/dns", "https://dns.example:/dns", "https://dns.example/%GG", "https://dns.example/%0d%0asecret", "https://dns.example/a b", "https://dns.example\\private", "https://127.1/dns", "https://[bad]/dns"]) assert.throws(() => validateCustomDohUrl(url));
});

test("private snapshots and save payloads are bounded and revision checked", () => {
  const snapshot = parseCustomDohSnapshot(envelope("dns.custom.inspect", data));
  assert.equal(snapshot.url, data.url);
  assert.deepEqual(buildCustomDohPayload(snapshot, data.url, true), { schema: 1, revision: data.revision, url: data.url, via_proxy: true });
  assert.equal(parseCustomDohSnapshot(envelope("dns.custom.inspect", { ...data, configured: false, url: null })).url, "");
  for (const value of [{ ...data, revision: "bad" }, { ...data, configured: false }, { ...data, via_proxy: "false" }, { ...data, url: "http://dns.example" }]) assert.equal(parseCustomDohSnapshot(envelope("dns.custom.inspect", value)), null);
  assert.equal(parseCustomDohSnapshot(envelope("dns.status", data)), null);
  assert.throws(() => buildCustomDohPayload({ ...snapshot, revision: "bad" }, data.url, false));
});

test("custom DoH public status and switch reports only expose a stable resolver tag", () => {
  const state = parseMachineDns(envelope("dns.status", { profile: "custom-doh", primary: "custom-profile-dns", secondary: null, transport: "doh", via_proxy: false, bootstrap_configured: "aliyun", bootstrap_transport: "doh", lan: { configured: false, effective: false, server: null, domains: [] } }));
  assert.equal(state.profile, "custom-doh");
  assert.equal(parseMachineDns(envelope("dns.status", { profile: "custom-doh", primary: "custom-profile-dns", secondary: null, transport: "doh", via_proxy: null, bootstrap_configured: "aliyun", bootstrap_transport: "doh", lan: { configured: false, effective: false, server: null, domains: [] } })).viaProxy, null);
  const plan = buildDnsProfilePlan("default", "custom-doh");
  assert.equal(plan.transport, "doh");
  assert.equal(plan.primary, "custom-profile-dns");
  assert.doesNotMatch(formatDnsProfilePlanReport(plan), /https:|private-fixture/);
});

test("the custom DoH form keeps endpoint drafts private and synchronizes translations", () => {
  const source = readFileSync(new URL("./src/components/pages/CustomDohSettings.vue", import.meta.url), "utf8");
  assert.match(source, /runPrivateCli\("--json dns custom inspect"/);
  assert.match(source, /stagePrivatePayload\("tmp", basename, payload/);
  assert.match(source, /dns custom save-file \[private-payload\]/);
  assert.match(source, /finally[\s\S]*removePrivatePayload/);
  assert.doesNotMatch(source, /state\.output|state\.notice|localStorage|sessionStorage|copyText/);
  assert.match(source, /if \(disposed \|\| !props.enabled\) return/);
  for (const match of source.matchAll(/\bt\(["']([^"']+)["']\)/g)) assert.ok(messages[match[1]]?.every(Boolean), match[1]);
});

test("custom DoH save finishes private cleanup before notifying the parent to refresh", async () => {
  const source = readFileSync(new URL("./src/components/pages/CustomDohSettings.vue", import.meta.url), "utf8");
  const script = ts.createSourceFile("CustomDohSettings.ts", source.match(/<script setup lang="ts">([\s\S]*?)<\/script>/)[1], ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const handler = script.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === "save");
  const compiled = ts.transpileModule(handler.getText(script), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  const events = [];
  const scope = {
    props: { enabled: true }, pendingPayload: { value: "private-fixture" }, busy: { value: false }, disposed: false,
    error: { value: "" }, status: { value: "" },
    withAction: async (_key, task) => { await task(); events.push("unlocked"); },
    stagePrivatePayload: async () => { events.push("stage"); return { path: "/private/payload", basename: "payload.json" }; },
    runPrivateCli: async () => { events.push("apply"); return { ok: true }; },
    removePrivatePayload: async () => { events.push("cleanup"); return true; },
    shellQuote: (value) => value, t: (value) => value,
    emit: (event) => events.push(event), load: async () => events.push("reload-editor"),
  };
  await runInNewContext(`${compiled}\nsave()`, scope);
  assert.deepEqual(events, ["stage", "apply", "cleanup", "unlocked", "saved"]);
});

test("the active custom profile does not force its editor open and has a translated edit action", () => {
  const source = readFileSync(new URL("./src/components/pages/DnsToolsCard.vue", import.meta.url), "utf8");
  assert.match(source, /<CustomDohSettings v-if="customDohSelected"/);
  assert.match(source, /@saved="onCustomDohSaved"/);
  assert.match(source, /:disabled="!dnsSettingsEnabled \|\| customDohBusy"/);
  assert.deepEqual(messages["编辑自定义 DoH"], ["Edit custom DoH", "Изменить свой DoH"]);
});
