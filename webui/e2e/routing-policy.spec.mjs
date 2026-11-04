import { expect, test } from "@playwright/test";

async function mount(page, failure = "") {
  await page.addInitScript(({ failure }) => {
    localStorage.setItem("magicnet.webui.onboarding.v1", "dismissed");
    const policy = {
      schema: 1, revision: "a".repeat(64), source: "defaults", protected_count: 3,
      rules: [
        { conditions: { rule_set: ["grok", "gemini"] }, target: "proxy-auto", available: true },
        { conditions: { domain_suffix: ["cn"] }, target: "direct", available: true },
      ], groups: [], final: "proxy-auto", rule_sets: ["grok", "gemini"],
      targets: [{ tag: "proxy", type: "selector" }, { tag: "proxy-auto", type: "urltest" },
        { tag: "final", type: "selector" }, { tag: "node-a", type: "socks" },
        { tag: "direct", type: "direct" }, { tag: "block", type: "block" }],
    };
    window.__routingFixture = { policy, failure, payload: "", saves: [], removals: 0, groupsReads: 0 };
    const envelope = (command, data) => JSON.stringify({ schema: 1, ok: true, command, data });
    window.ksu = {
      spawn(command, _args, _options, callbackName) {
        const callback = window[callbackName];
        setTimeout(() => {
          const fixture = window.__routingFixture;
          let stdout = "";
          let errno = 0;
          if (command.includes("--json capabilities")) stdout = envelope("machine.capabilities", {
            commands: ["routing.inspect"], private_commands: ["routing.inspect"],
            features: fixture.failure === "unsupported" ? [] : ["routing_policy_management"],
          });
          else if (command.includes("--json routing inspect")) stdout = envelope("routing.inspect", { policy: fixture.policy, rules: [], outbounds: [] });
          else if (command.includes("api proxies")) {
            fixture.groupsReads += 1;
            stdout = JSON.stringify({ proxies: {
              proxy: { type: "Selector", all: ["node-a", "proxy-auto"], now: "proxy-auto" },
              "proxy-auto": { type: "URLTest", all: ["node-a"], now: "node-a" },
              final: { type: "Selector", all: ["proxy-auto"], now: "proxy-auto" },
              "node-a": { type: "Socks" },
              ...Object.fromEntries(fixture.policy.groups.map((group) => [group.tag, {
                type: group.type === "selector" ? "Selector" : "URLTest", all: group.members,
                now: group.default || group.members[0],
              }])),
            } });
          } else if (command.includes("webui payload create tmp")) {
            fixture.payload = "";
            stdout = `/data/adb/modules/MagicNet/.tmp/webui-payload/${command.match(/routing-policy-[0-9]+-[a-z0-9]+\.json/)[0]}`;
          } else if (command.includes("webui payload append tmp")) {
            const chunks = command.match(/[a-zA-Z0-9+/]{32,}={0,2}/g);
            fixture.payload += new TextDecoder().decode(Uint8Array.from(atob(chunks.at(-1)), (character) => character.charCodeAt(0)));
          } else if (command.includes("webui payload remove tmp")) fixture.removals += 1;
          else if (command.includes("routing save-file")) {
            const payload = JSON.parse(fixture.payload);
            fixture.saves.push(payload);
            if (fixture.failure === "validation") { errno = 1; stdout = "private-validation-detail-fixture"; }
            else {
              fixture.policy = { ...fixture.policy, ...payload, source: "user_intent", revision: "b".repeat(64),
                targets: [...fixture.policy.targets.filter((target) => !fixture.policy.groups.some((group) => group.tag === target.tag)),
                  ...payload.groups.map((group) => ({ tag: group.tag, type: group.type }))] };
              stdout = "[info] Routing policy validated, saved and applied";
            }
          }
          if (stdout) callback.stdout.emit("data", stdout);
          callback.emit("exit", errno);
        }, command.includes("routing save-file") || command.includes("webui payload remove tmp") ? 100 : 0);
      },
    };
  }, { failure });
  await page.goto("/#/proxy", { waitUntil: "domcontentloaded" });
  const panel = page.locator(".routing-policy-panel");
  await panel.waitFor({ state: "attached", timeout: 30_000 });
  await panel.getByRole("heading", { name: "路由规则", exact: true }).scrollIntoViewIfNeeded({ timeout: 15_000 });
  if (failure !== "unsupported") await expect(panel.locator(".routing-rule-row")).toHaveCount(2);
  return panel;
}

async function apply(panel) {
  await panel.getByRole("button", { name: "保存并应用", exact: true }).click();
  await panel.getByRole("button", { name: "确认应用", exact: true }).click();
}

test("routing rules support direct node choice, search, ordering and confirmed deletion", async ({ page }) => {
  const panel = await mount(page);
  await expect(panel.getByText("已保留 3 条系统防护规则，优先于用户业务规则执行。")).toBeVisible();
  await panel.getByRole("button", { name: "添加路由规则", exact: true }).click();
  await panel.getByLabel("匹配条件（JSON）", { exact: true }).fill('{"domain_suffix":["private-routing-fixture.invalid"]}');
  await panel.getByRole("combobox", { name: "目标出站", exact: true }).selectOption("node-a");
  await panel.getByRole("button", { name: "保留规则修改", exact: true }).click();
  await expect(panel.locator(".routing-rule-row")).toHaveCount(3);
  await panel.getByRole("button", { name: "上移规则 3", exact: true }).click();
  await panel.getByLabel("搜索路由规则", { exact: true }).fill("private-routing-fixture");
  await expect(panel.locator(".routing-rule-row")).toHaveCount(1);
  await expect(panel.getByRole("button", { name: "上移规则 2", exact: true })).toBeDisabled();
  await panel.getByLabel("搜索路由规则", { exact: true }).fill("");
  await panel.getByRole("button", { name: "删除规则 3", exact: true }).click();
  await expect(panel.locator(".routing-rule-row")).toHaveCount(3);
  await panel.getByRole("button", { name: "确认删除", exact: true }).click();
  await expect(panel.locator(".routing-rule-row")).toHaveCount(2);
  await apply(panel);
  await expect(panel.getByText("路由策略已保存并应用，订阅刷新会保留用户设置。")).toBeVisible();
  const fixture = await page.evaluate(() => window.__routingFixture);
  expect(fixture.saves[0].rules[1].target).toBe("node-a");
  expect(fixture.saves[0].rules[1].conditions.domain_suffix).toEqual(["private-routing-fixture.invalid"]);
  expect(fixture.removals).toBe(1);
  const retained = await page.evaluate(async () => {
    const { state } = (await import("/src/composables/useMagicNet.ts")).useMagicNet();
    return JSON.stringify([state.output, state.notice, state.lastCommand, state.operationCapture, localStorage, sessionStorage]);
  });
  expect(retained).not.toContain("private-routing-fixture");
});

test("optional service groups are reusable, rename references atomically and refresh selection tabs", async ({ page }) => {
  const panel = await mount(page);
  await panel.getByRole("button", { name: "添加策略组", exact: true }).click();
  await panel.getByLabel("策略组名称", { exact: true }).fill("ai-grok");
  await panel.getByRole("listbox", { name: "策略组成员（可多选）", exact: true }).selectOption(["node-a", "proxy-auto"]);
  await panel.getByRole("combobox", { name: "默认出站", exact: true }).selectOption("node-a");
  await panel.getByRole("button", { name: "保留策略组修改", exact: true }).click();
  await panel.getByRole("button", { name: "编辑规则 1", exact: true }).click();
  await panel.getByRole("combobox", { name: "目标出站", exact: true }).selectOption("ai-grok");
  await panel.getByRole("button", { name: "保留规则修改", exact: true }).click();
  await expect(panel.getByRole("button", { name: "删除策略组 ai-grok", exact: true })).toBeDisabled();
  await panel.getByRole("button", { name: "编辑策略组 ai-grok", exact: true }).click();
  await panel.getByLabel("策略组名称", { exact: true }).fill("my-ai");
  await panel.getByRole("button", { name: "保留策略组修改", exact: true }).click();
  await expect(panel.locator(".routing-rule-row").first()).toContainText("my-ai");
  await apply(panel);
  await expect(page.getByRole("tab", { name: /my-ai/ })).toBeVisible();
  const fixture = await page.evaluate(() => window.__routingFixture);
  expect(fixture.saves[0].groups[0].tag).toBe("my-ai");
  expect(fixture.saves[0].rules[0].target).toBe("my-ai");
});

test("failed application retains drafts and cleans private temporary payloads", async ({ page }) => {
  const panel = await mount(page, "validation");
  await panel.getByRole("button", { name: "编辑规则 1", exact: true }).click();
  await panel.getByRole("combobox", { name: "目标出站", exact: true }).selectOption("block");
  await panel.getByRole("button", { name: "保留规则修改", exact: true }).click();
  await apply(panel);
  await expect(panel.getByRole("alert")).toContainText("路由策略校验或应用失败");
  await expect(panel.locator(".routing-rule-row").first()).toContainText("block");
  await expect(panel.getByText("修改尚未保存。", { exact: true })).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__routingFixture.removals)).toBe(1);
});

test("old modules cannot silently fall back to legacy routing writes", async ({ page }) => {
  const panel = await mount(page, "unsupported");
  await expect(panel.getByRole("alert")).toContainText("当前模块不支持路由策略管理");
  await expect(panel.getByRole("button", { name: "添加路由规则", exact: true })).toHaveCount(0);
});


test("editing and confirmation lock structural mutations against positional drift", async ({ page }) => {
  const panel = await mount(page);
  await panel.getByRole("button", { name: "编辑规则 1", exact: true }).click();
  await expect(panel.getByRole("button", { name: "删除规则 1", exact: true })).toBeDisabled();
  await expect(panel.getByRole("button", { name: "上移规则 2", exact: true })).toBeDisabled();
  await expect(panel.getByRole("combobox", { name: "兜底出站", exact: true })).toBeDisabled();
  await panel.getByRole("button", { name: "取消", exact: true }).click();
  await panel.getByRole("button", { name: "删除规则 1", exact: true }).click();
  await expect(panel.getByRole("button", { name: "删除规则 2", exact: true })).toBeDisabled();
  await expect(panel.getByRole("button", { name: "添加路由规则", exact: true })).toBeDisabled();
  await panel.getByRole("button", { name: "确认删除", exact: true }).click();
  await expect(panel.locator(".routing-rule-row")).toHaveCount(1);
  await expect(panel.locator(".routing-rule-row")).toContainText("cn");
});
