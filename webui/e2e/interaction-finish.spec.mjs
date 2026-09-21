import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem("magicnet.webui.onboarding.v1", "dismissed");
    localStorage.setItem("magicnet.webui.theme", "light");
  });
  await page.goto("/", { waitUntil: "networkidle" });
  await expect(page.locator(".mn-control")).toBeVisible();
});

async function setTask(page, task) {
  // Display-only fixture: never install a bridge or execute root commands.
  await page.evaluate(async (task) => {
    const { useMagicNet } = await import("/src/composables/useMagicNet.ts");
    useMagicNet().state.task = task;
  }, task);
}

test("busy buttons keep their geometry and show explicit progress", async ({ page }) => {
  const buttons = [
    ["刷新面板", "刷新面板", true],
    ["创建 GitHub Issue", "创建 GitHub issue", false],
  ];
  for (const [index, [label, task, iconOnly]] of buttons.entries()) {
    const initialButton = page.getByRole("button", { name: label, exact: true });
    if (!(await initialButton.isVisible())) continue; // Feedback lives in the desktop toolbar.
    await initialButton.evaluate((element, id) => {
      element.dataset.e2eBusyButton = String(id);
    }, index);
    const button = page.locator(`[data-e2e-busy-button="${index}"]`);
    const before = await button.boundingBox();
    await setTask(page, task);
    await expect(button).toHaveAttribute("aria-busy", "true");
    await expect(button).toBeDisabled();
    if (!iconOnly) await expect(button).toContainText("执行中");
    const during = await button.boundingBox();
    for (const key of ["x", "y", "width", "height"])
      expect(Math.abs(during[key] - before[key])).toBeLessThan(1);
    await expect(button.locator(".mn-button__content")).toHaveCSS("visibility", "hidden");
    await setTask(page, "");
    await expect(button).toBeEnabled();
    await expect(button.locator(".mn-button__content")).toHaveCSS("visibility", "visible");
  }
});

test("async click handlers automatically lock and restore their button", async ({ page }) => {
  await page.addInitScript(() => {
    window.__clipboardWrites = 0;
    window.__resolveClipboard = null;
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText() {
          window.__clipboardWrites += 1;
          return new Promise((resolve) => {
            window.__resolveClipboard = resolve;
          });
        },
      },
    });
  });
  await page.reload({ waitUntil: "networkidle" });
  await page.evaluate(async () => {
    const { state } = (await import("/src/composables/useMagicNet.ts")).useMagicNet();
    state.output = "button pending fixture";
  });

  const desktopOutput = page.locator('.desktop-rail [data-tab="output"]');
  if (await desktopOutput.isVisible()) {
    await desktopOutput.click();
  } else {
    await page.locator('[data-workspace="toolbox"]:visible').click();
    await page.locator('.mn-section-tabs [data-tab="output"]:visible').click();
  }

  const button = page.locator(".page-surface .mn-button").filter({ hasText: "复制脱敏输出" }).first();
  await button.click();
  await expect(button).toHaveAttribute("aria-busy", "true");
  await expect(button).toBeDisabled();
  await expect(button).toContainText("执行中");
  await button.dispatchEvent("click");
  expect(await page.evaluate(() => window.__clipboardWrites)).toBe(1);

  await page.evaluate(() => window.__resolveClipboard?.());
  await expect(button).not.toHaveAttribute("aria-busy", "true");
  await expect(button).toBeEnabled();
  expect(await page.evaluate(() => window.__clipboardWrites)).toBe(1);
});

test("navigation and disclosure focus remain distinct in both themes", async ({ page }, testInfo) => {
  for (const theme of ["light", "dark"]) {
    if (theme === "dark") await page.getByRole("button", { name: /外观主题/ }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    const mobile = await page.locator(".mobile-nav").isVisible();
    const active = page.locator(mobile ? ".mobile-nav .mn-nav-active" : ".desktop-rail .mn-nav-active");
    await expect(active).toHaveCount(1);
    const colors = await active.evaluate((el) => [getComputedStyle(el).backgroundColor, getComputedStyle(document.body).backgroundColor]);
    expect(colors[0]).not.toBe(colors[1]);
    const summary = page.locator(".mn-disclosure > summary").first();
    await page.keyboard.press("Tab");
    await summary.focus();
    await expect(summary).toBeFocused();
    await expect(summary).toHaveCSS("outline-style", "solid");
    await expect(summary).toHaveCSS("outline-width", "2px");
    await page.screenshot({ path: testInfo.outputPath(`paper-${theme}.png`), fullPage: false });
  }
});
