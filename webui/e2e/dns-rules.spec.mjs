import { expect, test } from "@playwright/test";

async function mount(page, failure = "", route = "/#/settings/dns") {
  await page.addInitScript(({ failure }) => {
    localStorage.setItem("magicnet.webui.onboarding.v1", "dismissed");
    const application = "__magicnet_dns_profile__";
    const bootstrap = "bootstrap-local-dns";
    const data = {
      revision: "a".repeat(64), source: "configuration", truncated: false, runtime_state: "unknown",
      rules: [
        { id: 1, conditions: { clash_mode: "Global" }, target: application, editable: false, protected_reason: "mode" },
        { id: 2, conditions: { domain_suffix: ["cn"] }, target: bootstrap, editable: true, protected_reason: null },
        { id: 3, conditions: { rule_set: ["private-rule-set-fixture"] }, target: application, editable: true, protected_reason: null },
      ],
      resolvers: [{ tag: application, kind: "application", available: true }, { tag: bootstrap, kind: "bootstrap", available: true }],
      rule_sets: ["private-rule-set-fixture"], final_server: "cloudflare-profile-dns", node_resolver: bootstrap, application_server: "cloudflare-profile-dns",
    };
    window.__dnsRules = { data, payload: "", saves: 0, removals: 0, inspections: 0, failure };
    const envelope = (command, value) => JSON.stringify({ schema: 1, ok: true, command, data: value });
    window.ksu = {
      spawn(command, _args, _options, callbackName) {
        setTimeout(() => {
          const callback = window[callbackName];
          const fixture = window.__dnsRules;
          let output = "";
          let errno = 0;
          if (command.includes("--json capabilities")) output = envelope("machine.capabilities", { commands: failure === "unsupported" ? [] : ["dns.inspect"], private_commands: ["dns.inspect"] });
          else if (command.includes("--json dns inspect")) { fixture.inspections += 1; output = envelope("dns.inspect", fixture.data); }
          else if (command.includes("--json dns status")) output = envelope("dns.status", { profile: "default", primary: bootstrap, secondary: null, transport: "default", via_proxy: false, bootstrap_configured: "aliyun", bootstrap_transport: "doh", lan: { configured: false, effective: false, server: null, domains: [] } });
          else if (command.includes("--json network status")) output = envelope("network.status", { configured: { ipv6_mode: "prefer_ipv4", mtu: 1400, udp_timeout: "5m", dns_interception: "on" }, effective: { ipv6_mode: "prefer_ipv4", stack: "mixed", mtu: 1400, udp_timeout: "5m", dns_interception: "on" } });
          else if (command.includes("webui payload create tmp")) {
            fixture.payload = "";
            output = `/data/adb/modules/MagicNet/.tmp/webui-payload/${command.match(/dns-rules-[0-9]+-[a-z0-9]+\.json/)[0]}`;
          } else if (command.includes("webui payload append tmp")) {
            const chunks = command.match(/[a-zA-Z0-9+/]{32,}={0,2}/g);
            fixture.payload += new TextDecoder().decode(Uint8Array.from(atob(chunks.at(-1)), (char) => char.charCodeAt(0)));
          } else if (command.includes("webui payload remove tmp")) fixture.removals += 1;
          else if (command.includes("dns rules save-file")) {
            fixture.saves += 1;
            if (fixture.failure === "validation") { errno = 1; output = "validator-private-detail-fixture"; }
            else {
              const payload = JSON.parse(fixture.payload);
              fixture.data.rules = payload.rules.map((rule, index) => rule.conditions
                ? { id: index + 1, conditions: rule.conditions, target: rule.target, editable: true, protected_reason: null }
                : { ...fixture.data.rules[rule.id - 1], id: index + 1 });
              fixture.data.source = "user_intent";
              fixture.data.revision = "b".repeat(64);
              output = "[info] DNS rules validated, saved and applied";
            }
          }
          if (output) callback.stdout.emit("data", output);
          callback.emit("exit", errno);
        }, command.includes("dns rules save-file") ? 120 : 0);
      },
    };
  }, { failure });
  await page.goto(route, { waitUntil: "domcontentloaded" });
  const panel = page.locator(".dns-rules-panel");
  await panel.waitFor({ state: "attached", timeout: 30_000 });
  await expect(page).toHaveURL(/#\/settings\/dns$/);
  await panel.scrollIntoViewIfNeeded();
  if (failure !== "unsupported") await expect(panel.locator(".dns-rule-row")).toHaveCount(3);
  return panel;
}

async function edit(page, panel, conditions = '{"domain_suffix":["edited.invalid"]}') {
  await panel.locator(".dns-rule-row").nth(1).getByRole("button", { name: "调整", exact: true }).click();
  await page.getByLabel("匹配条件（JSON）").fill(conditions);
  await panel.getByRole("combobox", { name: "DNS 解析目标", exact: true }).selectOption("__magicnet_dns_profile__");
  await panel.getByRole("button", { name: "保留规则修改", exact: true }).click();
}

async function expectPrivateTransport(page) {
  const retained = await page.evaluate(async () => {
    const { state } = (await import("/src/composables/useMagicNet.ts")).useMagicNet();
    return [state.output, state.notice, state.lastCommand, JSON.stringify(state.operationCapture), JSON.stringify(localStorage), JSON.stringify(sessionStorage)].join("\n");
  });
  expect(retained).not.toContain("private-rule-set-fixture");
  expect(retained).not.toContain("validator-private-detail-fixture");
}

test("DNS rules show protected boundaries and distinguish Bootstrap from the application profile", async ({ page }) => {
  const panel = await mount(page, "", "/#/dns");
  await expect(page.locator(".mn-detail-head h2")).toHaveText("DNS 设置");
  await expect(panel.locator(".dns-rule-row").first().getByText("模式规则由模块维护")).toBeVisible();
  await expect(panel.locator(".dns-rule-row").first().getByRole("button")).toHaveCount(0);
  await expect(panel.getByRole("button", { name: "上移第 2 条 DNS 规则", exact: true })).toBeDisabled();
  await expect(panel.getByText("节点域名默认解析器（只读）", { exact: false })).toBeVisible();
  await expect(panel.locator(".dns-rule-row").nth(1)).toContainText("Bootstrap DNS");
  await expect(panel.locator(".dns-rule-row").nth(2)).toContainText("应用 DNS Profile");
  await expect(page.getByRole("option", { name: "跟随 Bootstrap DNS", exact: true })).toHaveAttribute("value", "default");
  await expectPrivateTransport(page);
});

test("edits and reorders ordinary rules then privately validates and applies once", async ({ page }) => {
  const panel = await mount(page);
  await edit(page, panel);
  expect(await page.evaluate(() => window.__dnsRules.saves)).toBe(0);
  await panel.getByRole("button", { name: "下移第 2 条 DNS 规则", exact: true }).click();
  await panel.getByRole("button", { name: "校验并应用 DNS 规则", exact: true }).click();
  await panel.getByRole("button", { name: "确认执行", exact: true }).click();
  await expect(panel.getByRole("status")).toHaveText("DNS 规则已校验、保存并应用。");
  const fixture = await page.evaluate(() => ({ payload: JSON.parse(window.__dnsRules.payload), saves: window.__dnsRules.saves, removals: window.__dnsRules.removals }));
  expect(fixture.saves).toBe(1);
  expect(fixture.removals).toBe(1);
  expect(fixture.payload.rules[0]).toEqual({ id: 1 });
  expect(fixture.payload.rules[2].conditions.domain_suffix).toEqual(["edited.invalid"]);
  expect(fixture.payload.rules[2].target).toBe("__magicnet_dns_profile__");
  await expectPrivateTransport(page);
});

test("invalid conditions are rejected before payload creation or device mutation", async ({ page }) => {
  const panel = await mount(page);
  await edit(page, panel, '{"rule_set":["missing"]}');
  await expect(panel.getByRole("alert")).toContainText("不存在的 rule-set");
  expect(await page.evaluate(() => window.__dnsRules.saves)).toBe(0);
  expect(await page.evaluate(() => window.__dnsRules.payload)).toBe("");
});

test("adding and deleting a draft rule does not mutate the device", async ({ page }) => {
  const panel = await mount(page);
  await panel.getByRole("button", { name: "添加 DNS 规则", exact: true }).click();
  await page.getByLabel("匹配条件（JSON）").fill('{"domain_suffix":["new.invalid"]}');
  await panel.getByRole("combobox", { name: "DNS 解析目标", exact: true }).selectOption("bootstrap-local-dns");
  await panel.getByRole("button", { name: "保留规则修改", exact: true }).click();
  await expect(panel.locator(".dns-rule-row")).toHaveCount(4);
  await expect(panel.locator(".dns-rule-row").last()).toContainText("Bootstrap DNS");
  await panel.getByRole("button", { name: "删除第 4 条 DNS 规则", exact: true }).click();
  await expect(panel.locator(".dns-rule-row")).toHaveCount(3);
  expect(await page.evaluate(() => window.__dnsRules.saves)).toBe(0);
});

test("device validation failure preserves edits, cleans staging and hides raw output", async ({ page }) => {
  const panel = await mount(page, "validation");
  await edit(page, panel);
  await panel.getByRole("button", { name: "校验并应用 DNS 规则", exact: true }).click();
  await panel.getByRole("button", { name: "确认执行", exact: true }).click();
  await expect(panel.getByRole("alert")).toContainText("未确认生效");
  await expect(panel.getByRole("button", { name: "放弃未保存修改", exact: true })).toBeEnabled();
  expect(await page.evaluate(() => window.__dnsRules.removals)).toBe(1);
  expect(await page.locator("body").innerText()).not.toContain("validator-private-detail-fixture");
  await expectPrivateTransport(page);
});

test("unsupported modules do not fall back to parsing human CLI output", async ({ page }) => {
  const panel = await mount(page, "unsupported");
  await expect(panel.getByRole("alert")).toContainText("请升级模块");
  await expect(panel.getByRole("button", { name: "添加 DNS 规则", exact: true })).toBeDisabled();
  expect(await page.evaluate(() => window.__dnsRules.inspections)).toBe(0);
});

test("DNS settings are a separate hub item and proxy settings contain no DNS controls", async ({ page }) => {
  test.setTimeout(90_000);
  await mount(page);
  const surface = page.locator('.page-surface[data-page="settings"]');
  await expect(page.locator(".mn-detail-head h2")).toHaveText("DNS 设置");
  for (const label of ["DNS 劫持", "应用 DNS Profile", "Bootstrap DNS"]) {
    await expect(surface.getByRole("combobox", { name: label, exact: true })).toBeVisible();
  }
  await expect(surface.getByRole("button", { name: "保存局域网 DNS", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "返回设置", exact: true }).click();
  await expect(page).toHaveURL(/#\/settings$/);
  await expect(page.getByRole("button", { name: /^DNS 设置/ })).toBeVisible();
  await page.getByRole("button", { name: /^代理设置/ }).click();
  await expect(page).toHaveURL(/#\/settings\/proxy$/);
  await expect(surface.getByRole("heading", { name: "协议栈", exact: true })).toBeVisible({ timeout: 30_000 });
  await expect(surface.locator(".dns-rules-panel")).toHaveCount(0);
  for (const label of ["DNS 劫持", "应用 DNS Profile", "Bootstrap DNS"]) {
    await expect(surface.getByRole("combobox", { name: label, exact: true })).toHaveCount(0);
  }
  await expect(surface.getByRole("button", { name: "保存局域网 DNS", exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "返回设置", exact: true }).click();
  await page.getByRole("button", { name: /^DNS 设置/ }).click();
  await expect(page).toHaveURL(/#\/settings\/dns$/);
  await surface.locator(".dns-rules-panel").scrollIntoViewIfNeeded();
  await expect(surface.locator(".dns-rule-row")).toHaveCount(3);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
});

for (const [locale, title, back] of [
  ["en-US", "DNS settings", "Back to settings"],
  ["ru-RU", "Настройки DNS", "Назад к настройкам"],
]) {
  test.describe(locale, () => {
    test.use({ locale });
    test("independent DNS settings translate their title and hub entry", async ({ page }) => {
      await mount(page);
      await expect(page.locator(".mn-detail-head h2")).toHaveText(title);
      await page.getByRole("button", { name: back, exact: true }).click();
      const entry = page.getByRole("button", { name: new RegExp(`^${title}`) });
      await expect(entry).toBeVisible();
      expect(await entry.innerText()).not.toMatch(/\p{Script=Han}/u);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
    });
  });
}
