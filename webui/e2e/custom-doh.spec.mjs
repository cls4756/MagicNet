import { expect, test } from "@playwright/test";

async function mount(page, failure = "", { initialProfile = "default", openEditor = true } = {}) {
  await page.addInitScript(({ failure, initialProfile }) => {
    localStorage.setItem("magicnet.webui.onboarding.v1", "dismissed");
    window.__customDoh = { failure, profile: initialProfile, payload: "", saves: 0, removals: 0, configured: initialProfile === "custom-doh", url: initialProfile === "custom-doh" ? "https://resolver.example/private-doh-fixture" : null, via_proxy: true, revision: "a".repeat(64), commands: [] };
    const envelope = (command, data) => JSON.stringify({ schema: 1, ok: true, command, data });
    window.ksu = {
      spawn(command, _args, _options, callbackName) {
        setTimeout(() => {
          const fixture = window.__customDoh;
          fixture.commands.push(command);
          const preset = command.match(/\bdns set .*?\b(default|cloudflare-doh|cloudflare-dot|cloudflare-udp)\b/);
          let output = "";
          let errno = 0;
          if (command.includes("--json capabilities")) output = envelope("machine.capabilities", { commands: failure === "unsupported" ? [] : ["dns.custom.inspect"], private_commands: ["dns.custom.inspect"], features: ["custom_doh"] });
          else if (command.includes("--json dns custom inspect")) output = envelope("dns.custom.inspect", { configured: fixture.configured, url: fixture.url, via_proxy: fixture.via_proxy, revision: fixture.revision });
          else if (command.includes("--json dns status")) output = envelope("dns.status", { profile: fixture.profile, primary: fixture.profile === "custom-doh" ? "custom-profile-dns" : fixture.profile === "default" ? "bootstrap-local-dns" : "cloudflare-profile-dns", secondary: null, transport: fixture.profile === "default" ? "default" : fixture.profile === "cloudflare-dot" ? "dot" : fixture.profile === "cloudflare-udp" ? "udp" : "doh", via_proxy: fixture.profile === "custom-doh" ? fixture.via_proxy : fixture.profile !== "default", bootstrap_configured: "aliyun", bootstrap_transport: "doh", lan: { configured: false, effective: false, server: null, domains: [] } });
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
              fixture.profile = "custom-doh";
              fixture.url = payload.url;
              fixture.via_proxy = payload.via_proxy;
              fixture.revision = "b".repeat(64);
              output = "[info] Custom DoH validated, saved and applied";
            }
          } else if (command.includes("webui payload remove tmp")) {
            fixture.removals += 1;
            if (failure === "cleanup") errno = 1;
          } else if (preset) {
            if (failure === "preset") { errno = 1; output = "DNS profile application failed"; }
            else { fixture.profile = preset[1]; output = "DNS profile saved and applied"; }
          }
          const callback = window[callbackName];
          if (output) callback.stdout.emit("data", output);
          if (errno) callback.stderr.emit("data", "validator-private-detail-fixture");
          callback.emit("exit", errno);
        }, 5);
      },
    };
  }, { failure, initialProfile });
  await page.goto("/#/settings/dns");
  const profile = page.getByRole("combobox", { name: "应用 DNS Profile", exact: true });
  await expect(profile).toBeEnabled({ timeout: 30_000 });
  await expect(profile).toHaveValue(initialProfile);
  const panel = page.getByRole("region", { name: "自定义 DoH", exact: true });
  if (!openEditor) return panel;
  await profile.selectOption("custom-doh");
  await expect(panel).toBeVisible();
  if (failure !== "unsupported") await expect(panel.getByLabel("DoH 地址", { exact: true })).toBeEnabled();
  return panel;
}

async function currentProfile(page) {
  return page.evaluate(async () => (await import("/src/composables/useMagicNet.ts")).useMagicNet().state.dns.profile);
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
  await expect(panel).toHaveCount(0);
  await expect.poll(() => currentProfile(page)).toBe("custom-doh");
  const fixture = await page.evaluate(() => ({ payload: JSON.parse(window.__customDoh.payload), saves: window.__customDoh.saves, removals: window.__customDoh.removals }));
  expect(fixture.payload.via_proxy).toBe(false);
  expect(fixture.payload.revision).toBe("a".repeat(64));
  expect(fixture.saves).toBe(1);
  expect(fixture.removals).toBe(1);
  await expect(page.getByRole("combobox", { name: "应用 DNS Profile", exact: true })).toHaveValue("custom-doh");
  await expect(page.getByRole("combobox", { name: "Bootstrap DNS", exact: true })).toHaveValue("aliyun");
  await page.getByRole("button", { name: "编辑自定义 DoH", exact: true }).click();
  await expect(panel.getByLabel("DoH 地址", { exact: true })).toBeEnabled();
  await expect(panel.getByLabel("DoH 地址", { exact: true })).toHaveValue(fixture.payload.url);
  await panel.getByRole("button", { name: "校验并应用自定义 DoH", exact: true }).click();
  await panel.getByRole("button", { name: "确认执行", exact: true }).click();
  await expect(panel).toHaveCount(0);
  expect(await page.evaluate(() => JSON.parse(window.__customDoh.payload).revision)).toBe("b".repeat(64));
  await page.getByRole("combobox", { name: "应用 DNS Profile", exact: true }).selectOption("cloudflare-doh");
  await page.getByRole("button", { name: "确认执行", exact: true }).click();
  await expect.poll(() => currentProfile(page)).toBe("cloudflare-doh");
  await expect(panel).toHaveCount(0);
  await expectPrivate(page);
});

for (const target of ["cloudflare-doh", "cloudflare-dot", "cloudflare-udp", "default"]) {
  test(`an applied custom DoH profile can switch to ${target} and retain its endpoint`, async ({ page }) => {
    const panel = await mount(page, "", { initialProfile: "custom-doh", openEditor: false });
    const profile = page.getByRole("combobox", { name: "应用 DNS Profile", exact: true });
    await expect(panel).toHaveCount(0);
    await profile.selectOption(target);
    await expect(profile).toHaveValue(target);
    await page.getByRole("button", { name: "确认执行", exact: true }).click();
    await expect.poll(() => currentProfile(page)).toBe(target);
    await expect(profile).toHaveValue(target);
    await expect(panel).toHaveCount(0);
    await profile.selectOption("custom-doh");
    await expect(panel.getByLabel("DoH 地址", { exact: true })).toHaveValue("https://resolver.example/private-doh-fixture");
    await expect(panel.getByLabel("DoH 地址", { exact: true })).toBeEnabled();
    await panel.getByRole("button", { name: "取消", exact: true }).click();
    await expect(panel).toHaveCount(0);
    await expect(profile).toHaveValue(target);
    await expectPrivate(page);
  });
}

test("canceling custom editing or a preset switch restores the applied profile", async ({ page }) => {
  const panel = await mount(page, "", { initialProfile: "custom-doh", openEditor: false });
  const profile = page.getByRole("combobox", { name: "应用 DNS Profile", exact: true });
  await page.getByRole("button", { name: "编辑自定义 DoH", exact: true }).click();
  await expect(panel.getByLabel("DoH 地址", { exact: true })).toBeEnabled();
  await panel.getByRole("button", { name: "取消", exact: true }).click();
  await expect(panel).toHaveCount(0);
  await expect(profile).toHaveValue("custom-doh");
  await profile.selectOption("default");
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await expect(profile).toHaveValue("custom-doh");
  await expect(panel).toHaveCount(0);
  await expect.poll(() => currentProfile(page)).toBe("custom-doh");
  await expectPrivate(page);
});

test("a failed preset switch keeps the applied custom profile without reopening its editor", async ({ page }) => {
  const panel = await mount(page, "preset", { initialProfile: "custom-doh", openEditor: false });
  const profile = page.getByRole("combobox", { name: "应用 DNS Profile", exact: true });
  await profile.selectOption("cloudflare-doh");
  await page.getByRole("button", { name: "确认执行", exact: true }).click();
  await expect(profile).toHaveValue("custom-doh");
  await expect(panel).toHaveCount(0);
  await expect.poll(() => currentProfile(page)).toBe("custom-doh");
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

test("unconfirmed private cleanup stays visible without blocking applied profile refresh", async ({ page }) => {
  const panel = await mount(page, "cleanup");
  await panel.getByLabel("DoH 地址", { exact: true }).fill("https://resolver.example/private-doh-fixture");
  await panel.getByRole("button", { name: "校验并应用自定义 DoH", exact: true }).click();
  await panel.getByRole("button", { name: "确认执行", exact: true }).click();
  await expect(panel.getByRole("alert")).toHaveText("自定义 DoH 私有临时数据清理未确认。");
  await expect.poll(() => currentProfile(page)).toBe("custom-doh");
  await expect(panel.getByLabel("DoH 地址", { exact: true })).toBeEnabled();
  await panel.getByRole("button", { name: "取消", exact: true }).click();
  await expect(panel).toHaveCount(0);
  await expect(page.getByRole("combobox", { name: "应用 DNS Profile", exact: true })).toHaveValue("custom-doh");
  await expectPrivate(page);
});

test("unsupported modules block custom DoH configuration writes", async ({ page }) => {
  const panel = await mount(page, "unsupported");
  await expect(panel.getByRole("alert")).toHaveText("当前模块不支持自定义 DoH，请升级模块后重试。");
  await expect(panel.getByRole("button", { name: "校验并应用自定义 DoH", exact: true })).toBeDisabled();
  expect(await page.evaluate(() => window.__customDoh.saves)).toBe(0);
});
