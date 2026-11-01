import { expect, test } from "@playwright/test";

async function mount(page, failure = "") {
  await page.addInitScript(({ failure }) => {
    localStorage.setItem("magicnet.webui.onboarding.v1", "dismissed");
    window.__customDoh = { failure, payload: "", saves: 0, removals: 0, configured: false, url: null, via_proxy: true, revision: "a".repeat(64), commands: [] };
    const envelope = (command, data) => JSON.stringify({ schema: 1, ok: true, command, data });
    window.ksu = {
      spawn(command, _args, _options, callbackName) {
        setTimeout(() => {
          const fixture = window.__customDoh;
          fixture.commands.push(command);
          let output = "";
          let errno = 0;
          if (command.includes("--json capabilities")) output = envelope("machine.capabilities", { commands: failure === "unsupported" ? [] : ["dns.custom.inspect"], private_commands: ["dns.custom.inspect"], features: ["custom_doh"] });
          else if (command.includes("--json dns custom inspect")) output = envelope("dns.custom.inspect", { configured: fixture.configured, url: fixture.url, via_proxy: fixture.via_proxy, revision: fixture.revision });
          else if (command.includes("--json dns status")) output = envelope("dns.status", { profile: fixture.configured ? "custom-doh" : "default", primary: fixture.configured ? "custom-profile-dns" : "bootstrap-local-dns", secondary: null, transport: fixture.configured ? "doh" : "default", via_proxy: fixture.via_proxy, bootstrap_configured: "aliyun", bootstrap_transport: "doh", lan: { configured: false, effective: false, server: null, domains: [] } });
          else if (command.includes("--json network status")) output = envelope("network.status", { configured: { ipv6_mode: "prefer_ipv4", mtu: 1400, udp_timeout: "5m", dns_interception: "on" }, effective: { ipv6_mode: "prefer_ipv4", stack: "mixed", mtu: 1400, udp_timeout: "5m", dns_interception: "on" } });
          else if (command.includes("webui payload create tmp")) {
            fixture.payload = "";
            output = `/data/adb/modules/MagicNet/.tmp/webui-payload/${command.match(/custom-doh-[0-9]+-[a-z0-9]+\.json/)[0]}`;
          } else if (command.includes("webui payload append tmp")) {
            const chunks = command.match(/[a-zA-Z0-9+/]{32,}={0,2}/g);
            fixture.payload += new TextDecoder().decode(Uint8Array.from(atob(chunks.at(-1)), (char) => char.charCodeAt(0)));
          } else if (command.includes("dns custom save-file")) {
            fixture.saves += 1;
            if (failure === "save") { errno = 1; output = "validator-private-detail-fixture"; }
            else {
              const payload = JSON.parse(fixture.payload);
              fixture.configured = true;
              fixture.url = payload.url;
              fixture.via_proxy = payload.via_proxy;
              fixture.revision = "b".repeat(64);
              output = "[info] Custom DoH validated, saved and applied";
            }
          } else if (command.includes("webui payload remove tmp")) fixture.removals += 1;
          const callback = window[callbackName];
          if (output) callback.stdout.emit("data", output);
          if (errno) callback.stderr.emit("data", "validator-private-detail-fixture");
          callback.emit("exit", errno);
        }, 5);
      },
    };
  }, { failure });
  await page.goto("/#/settings/dns");
  const profile = page.getByRole("combobox", { name: "应用 DNS Profile", exact: true });
  await expect(profile).toBeEnabled({ timeout: 30_000 });
  await profile.selectOption("custom-doh");
  const panel = page.getByRole("region", { name: "自定义 DoH", exact: true });
  await expect(panel).toBeVisible();
  if (failure !== "unsupported") await expect(panel.getByLabel("DoH 地址", { exact: true })).toBeEnabled();
  return panel;
}

async function expectPrivate(page) {
  const retained = await page.evaluate(async () => {
    const { state } = (await import("/src/composables/useMagicNet.ts")).useMagicNet();
    return [state.output, state.notice, state.lastCommand, JSON.stringify(state.operationCapture), JSON.stringify(localStorage), JSON.stringify(sessionStorage)].join("\n");
  });
  expect(retained).not.toContain("private-doh-fixture");
  expect(retained).not.toContain("validator-private-detail-fixture");
  const commands = await page.evaluate(() => window.__customDoh.commands.join("\n"));
  expect(commands).not.toContain("private-doh-fixture");
}

test("custom DoH privately saves an endpoint and direct route, retaining Bootstrap", async ({ page }) => {
  const panel = await mount(page);
  await panel.getByLabel("DoH 地址", { exact: true }).fill("https://resolver.example:8443/private-doh-fixture?key=fixture");
  await panel.getByRole("combobox", { name: "DoH 连接方式", exact: true }).selectOption({ label: "直连" });
  await panel.getByRole("button", { name: "校验并应用自定义 DoH", exact: true }).click();
  expect(await page.evaluate(() => window.__customDoh.saves)).toBe(0);
  await panel.getByRole("button", { name: "确认执行", exact: true }).click();
  await expect(panel.getByRole("status")).toHaveText("自定义 DoH 已校验、保存并应用。");
  await expect(panel.getByLabel("DoH 地址", { exact: true })).toBeEnabled();
  const fixture = await page.evaluate(() => ({ payload: JSON.parse(window.__customDoh.payload), saves: window.__customDoh.saves, removals: window.__customDoh.removals }));
  expect(fixture.payload.via_proxy).toBe(false);
  expect(fixture.payload.revision).toBe("a".repeat(64));
  expect(fixture.saves).toBe(1);
  expect(fixture.removals).toBe(1);
  await expect(page.getByRole("combobox", { name: "应用 DNS Profile", exact: true })).toHaveValue("custom-doh");
  await expect(page.getByRole("combobox", { name: "Bootstrap DNS", exact: true })).toHaveValue("aliyun");
  await expectPrivate(page);
});

test("invalid custom URLs do not stage data and confirmation can be canceled", async ({ page }) => {
  const panel = await mount(page);
  const field = panel.getByLabel("DoH 地址", { exact: true });
  await field.fill("http://resolver.example/dns-query");
  await panel.getByRole("button", { name: "校验并应用自定义 DoH", exact: true }).click();
  await expect(panel.getByRole("alert")).toHaveText("请输入有效的 HTTPS DoH 地址，不支持用户名、密码或片段。");
  await field.fill("https://resolver.example/private-doh-fixture");
  await panel.getByRole("button", { name: "校验并应用自定义 DoH", exact: true }).click();
  await panel.getByRole("button", { name: "取消", exact: true }).click();
  await expect(field).toHaveValue("https://resolver.example/private-doh-fixture");
  expect(await page.evaluate(() => window.__customDoh.saves)).toBe(0);
  await expectPrivate(page);
});

test("failed custom DoH application keeps the draft and redacts validator details", async ({ page }) => {
  const panel = await mount(page, "save");
  await panel.getByLabel("DoH 地址", { exact: true }).fill("https://resolver.example/private-doh-fixture");
  await panel.getByRole("button", { name: "校验并应用自定义 DoH", exact: true }).click();
  await panel.getByRole("button", { name: "确认执行", exact: true }).click();
  await expect(panel.getByRole("alert")).toHaveText("自定义 DoH 校验或应用失败。修改已保留，请重新读取配置或检查健康状态。");
  await expect(panel.getByLabel("DoH 地址", { exact: true })).toHaveValue("https://resolver.example/private-doh-fixture");
  expect(await page.evaluate(() => window.__customDoh.removals)).toBe(1);
  await expectPrivate(page);
});

test("unsupported modules block custom DoH configuration writes", async ({ page }) => {
  const panel = await mount(page, "unsupported");
  await expect(panel.getByRole("alert")).toHaveText("当前模块不支持自定义 DoH，请升级模块后重试。");
  await expect(panel.getByRole("button", { name: "校验并应用自定义 DoH", exact: true })).toBeDisabled();
  expect(await page.evaluate(() => window.__customDoh.saves)).toBe(0);
});
