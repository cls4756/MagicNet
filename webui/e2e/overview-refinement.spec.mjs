import { test, expect } from "@playwright/test";

async function fit(page) {
  expect(
    await page.evaluate(
      () =>
        document.documentElement.scrollWidth <=
        document.documentElement.clientWidth + 1,
    ),
  ).toBe(true);
  for (const button of await page
    .locator(".page-surface button:visible")
    .all()) {
    if (await button.getAttribute("aria-label") === "热点代理帮助") continue;
    const box = await button.boundingBox();
    expect(box.height).toBeGreaterThanOrEqual(32);
  }
}

for (const theme of ["light", "dark"]) {
  test(`runtime and subscription hierarchy: ${theme}`, async ({
    page,
  }, info) => {
    await page.addInitScript((theme) => {
      localStorage.setItem("magicnet.webui.onboarding.v1", "dismissed");
      localStorage.setItem("magicnet.webui.theme", theme);
    }, theme);
    await page.goto("/", { waitUntil: "networkidle" });
    await expect(page.locator(".mn-control")).toBeVisible();
    await page.evaluate(async () => {
      // Display fixture only: no root bridge is installed and no commands run.
      const { state } = (
        await import("/src/composables/useMagicNet.ts")
      ).useMagicNet();
      Object.assign(state, { hasKsu: true, busy: false, phase: "idle" });
      Object.assign(state.runtime, {
        singBoxState: "sing-box",
        singBox: "123",
        singBoxRssKib: 131072,
      });
      Object.assign(state.subscriptions, {
        configuredCount: 1,
        singBoxUrls: ["https://provider.example/subscription"],
        lastResult: "success",
        updateLockOwner: "none",
        lastImportedCount: 24,
        sourceUsage: [
          {
            id: "preview",
            index: 1,
            hostname: "provider.example",
            state: "cached",
            uploadBytes: 1073741824,
            downloadBytes: 2147483648,
            totalBytes: 107374182400,
            expireEpoch: null,
            updatedEpoch: 1700000000,
          },
        ],
      });
    });
    await expect(page.locator(".mn-control-metric").nth(2)).toContainText("128.0");
    await expect(page.locator(".mn-control-metric").nth(2)).toContainText("MiB");
    await fit(page);
    await page.screenshot({
      path: info.outputPath(`${theme}-runtime.png`),
      fullPage: true,
    });
    await page.evaluate(async () => {
      const { state } = (await import("/src/composables/useMagicNet.ts")).useMagicNet();
      state.runtime.serviceReady = false;
    });
    await expect(page.locator(".mn-control-service-value")).toContainText("服务未就绪");
    await page.screenshot({path:info.outputPath(`${theme}-service-not-ready.png`),fullPage:true});
    await page.evaluate(async () => {
      const { state } = (await import("/src/composables/useMagicNet.ts")).useMagicNet();
      state.runtime.serviceReady = null;
      state.runtime.singBoxRssKib = null;
    });
    await expect(page.locator(".mn-control-metric").nth(2)).toContainText("暂不可用");
    await page.evaluate(async () => {
      const { state } = (
        await import("/src/composables/useMagicNet.ts")
      ).useMagicNet();
      state.runtime.singBoxState = "stopped";
    });
    await expect(page.locator(".mn-control-metric").nth(2)).toContainText("—");
    await page.evaluate(() => { window.location.hash = '/subs'; });
    await expect(page.locator(".subscriptions-page")).toBeVisible();
    // Navigation performs a fresh read. Without a native bridge the prior
    // success must be cleared, not silently retained by the display fixture.
    await expect(page.locator(".update-outcome")).toHaveText("状态未知");
    await page.evaluate(async () => {
      const { state } = (await import("/src/composables/useMagicNet.ts")).useMagicNet();
      const { parseMachineSubscription } = await import("/src/composables/machineStatus.ts");
      const { subscriptionSnapshot } = await import("/machine-fixtures.mjs");
      state.subscriptions = parseMachineSubscription(subscriptionSnapshot({
        configuration: { sing_box_urls: ["https://provider.example/subscription"], user_agent: "", filters: [] },
        source_usage: [{ id: "ac46b9047eaebc40f857f85d6cff32c14307b4892925107d6b93e3f4691a1857", index: 1, hostname: "provider.example", state: "cached",
          upload_bytes: 1073741824, download_bytes: 2147483648, total_bytes: 107374182400,
          expire_epoch: null, updated_epoch: 1700000000 }],
      }));
    });
    await expect(page.locator(".update-outcome")).toHaveText("更新成功");
    await page.evaluate(async () => {
      const { state } = (await import("/src/composables/useMagicNet.ts")).useMagicNet();
      state.subscriptions.updateLockOwner = "unknown";
    });
    await expect(page.locator(".update-outcome")).toHaveText("状态未知");
    await page.evaluate(async () => {
      const { state } = (await import("/src/composables/useMagicNet.ts")).useMagicNet();
      state.subscriptions.updateLockOwner = "none";
    });
    await expect(page.locator(".update-outcome")).toHaveText("更新成功");
    await expect(page.locator(".subscription-source-list .source-card-identity > div > span")).toContainText("上次用量");
    await fit(page);
    await page.screenshot({
      path: info.outputPath(`${theme}-subscriptions.png`),
      fullPage: true,
    });
    await page.getByRole("button", { name: "编辑来源", exact: true }).click();
    await expect(page.locator(".source-card-input").first()).toBeFocused();
    await page.getByRole("button", { name: "取消", exact: true }).click();
    await expect(page.locator(".source-card-input")).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: "管理订阅", exact: true }),
    ).toBeFocused();
    await page.evaluate(async () => {
      const { state } = (
        await import("/src/composables/useMagicNet.ts")
      ).useMagicNet();
      state.subscriptions.updateRunning = true;
    });
    await expect(page.locator(".update-outcome")).toHaveText("更新中");
    await page.evaluate(async () => {
      const { state } = (
        await import("/src/composables/useMagicNet.ts")
      ).useMagicNet();
      state.subscriptions.updateRunning = false;
      state.subscriptions.lastResult = "failed";
      state.subscriptions.sourceUsage[0].totalBytes = null;
    });
    await expect(page.locator(".update-outcome")).toHaveText("更新未完成");
    await expect(page.locator(".subscription-feedback")).toContainText(
      "当前仍保留原有配置",
    );
    await expect(page.locator(".remaining-quota")).toHaveAttribute(
      "data-unavailable",
      "true",
    );
    await fit(page);
  });
}
