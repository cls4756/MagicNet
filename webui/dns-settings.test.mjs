import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { runInNewContext } from "node:vm";
import { SETTINGS_ROUTES } from "./src/components/pages/settingsRoutes.ts";
import { messages } from "./src/i18n/messages.ts";

const source = (path) => readFileSync(new URL(path, import.meta.url), "utf8");
const app = source("./src/App.vue");
const hub = source("./src/components/pages/SettingsHubPage.vue");

test("DNS has its own canonical settings route and retains legacy deep links", () => {
  assert.equal(SETTINGS_ROUTES.filter((route) => route === "dns").length, 1);
  assert.ok(SETTINGS_ROUTES.includes("proxy"));
  const targets = runInNewContext(`(${app.match(/const TAB_TARGETS:[^=]+= (\{[\s\S]*?\n\});/)[1]})`);
  assert.equal(targets.dns.workspace, "settings");
  assert.equal(targets.dns.settings, "dns");
  for (const legacy of ["stack", "domain", "block"]) {
    assert.equal(targets[legacy].settings, "proxy");
  }
});

test("settings navigation advertises DNS independently of proxy settings", () => {
  assert.match(hub, /key: "dns", title: "DNS 设置"/);
  assert.match(hub, /dns: defineAsyncComponent\(\(\) => import\("\.\/settings\/DnsSettingsPage\.vue"\)\)/);
  assert.match(hub, /dns: "DNS 设置"/);
  assert.match(hub, /key: "proxy", title: "代理设置", subtitle: "协议栈、域名转发与分流规则"/);
  assert.doesNotMatch(hub, /协议栈、DNS、域名转发与分流规则/);
});

test("the DNS detail owns all DNS controls while proxy retains only its responsibilities", () => {
  const dns = source("./src/components/pages/settings/DnsSettingsPage.vue");
  const content = source("./src/components/pages/SettingsDnsPage.vue");
  const card = source("./src/components/pages/DnsToolsCard.vue");
  const proxy = source("./src/components/pages/settings/ProxySettingsPage.vue");
  assert.match(dns, /<GroupSection>/);
  assert.match(dns, /<SettingsDnsPage\s*\/>/);
  assert.match(content, /<DnsToolsCard\s*\/>/);
  assert.match(content, /<DnsRulesPanel\s*\/>/);
  for (const label of ["DNS 劫持", "应用 DNS Profile", "Bootstrap DNS", "局域网 DNS"]) {
    assert.ok(card.includes(label), label);
  }
  assert.doesNotMatch(proxy, /SettingsDnsPage|DnsToolsCard|DnsRulesPanel/);
  for (const page of ["SettingsStackPage", "SettingsDomainForwardPage", "BlocklistPage"]) {
    assert.match(proxy, new RegExp(`<${page}\\s*\\/>`));
  }
});

test("DNS state is warmed from DNS settings, not from the proxy detail", () => {
  const warmSource = hub.match(/function warmDetail\(route: SettingsRoute \| null\): void \{[\s\S]*?\n\}/)[0]
    .replace("route: SettingsRoute | null", "route").replace("): void", ")");
  const refreshers = ["refreshNetwork", "refreshDns", "refreshDomainForward", "refreshBlock", "refreshApps", "refreshWarp", "refreshMcp", "refreshHealth"];
  function warmed(route) {
    const calls = [];
    const scope = Object.fromEntries(refreshers.map((name) => [name, (force) => {
      assert.equal(force, true);
      calls.push(name);
    }]));
    runInNewContext(warmSource, scope);
    scope.warmDetail(route);
    return calls;
  }
  assert.deepEqual(warmed("dns"), ["refreshNetwork", "refreshDns"]);
  assert.deepEqual(warmed("proxy"), ["refreshNetwork", "refreshDomainForward", "refreshBlock"]);
});

test("new DNS navigation and updated descriptions have English and Russian translations", () => {
  assert.deepEqual(messages["DNS 设置"], ["DNS settings", "Настройки DNS"]);
  assert.deepEqual(messages["DNS 配置"], ["DNS configuration", "Конфигурация DNS"]);
  for (const key of ["协议栈、域名转发与分流规则", "DNS 劫持、应用 DNS、Bootstrap DNS 与解析规则"]) {
    assert.equal(messages[key].length, 2);
    assert.ok(messages[key].every((translation) => translation && !/\p{Script=Han}/u.test(translation)));
  }
});
