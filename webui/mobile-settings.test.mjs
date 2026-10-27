import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { runInNewContext } from "node:vm";
const app = readFileSync(new URL("./src/App.vue", import.meta.url), "utf8");
const css = readFileSync(new URL("./src/styles.css", import.meta.url), "utf8");

test("five clicks reveal GPT-6, retain previous visitors, and wrap the rotation", () => {
  const visitors = runInNewContext(
    app.match(/const easterEggVisitors = ([\s\S]*?) as const;/)[1],
  );
  assert.equal(
    visitors.map(({ name }) => name).join(","),
    "GPT-6,SOL,Grok 4.5,Kimi K3,Fable 5",
  );
  let now = 1000;
  const scope = {
    Date: { now: () => now },
    window: { clearTimeout() {}, setTimeout: () => 1 },
    closeEasterEgg() {},
    easterEggVisitors: visitors,
    brandClickCount: 0,
    brandClickWindowStartedAt: 0,
    easterEggNextIndex: 0,
    easterEggShownIndex: { value: 0 },
    easterEggVisible: { value: false },
    easterEggTimer: undefined,
  };
  runInNewContext(
    app
      .match(/function handleBrandMarkClick\(\): void \{[\s\S]*?\n\}/)[0]
      .replace(": void", ""),
    scope,
  );
  for (let i = 0; i < 4; i++) scope.handleBrandMarkClick();
  assert.equal(scope.easterEggVisible.value, false);
  scope.handleBrandMarkClick();
  assert.equal(scope.easterEggVisible.value, true);
  assert.equal(scope.easterEggShownIndex.value, 0);
  for (let i = 1; i <= visitors.length; i++) {
    for (let click = 0; click < 5; click++) scope.handleBrandMarkClick();
    assert.equal(scope.easterEggShownIndex.value, i % visitors.length);
  }
  scope.handleBrandMarkClick();
  now += 2500;
  scope.handleBrandMarkClick();
  assert.equal(scope.brandClickCount, 1);
});

test("run-page layout is scoped and short viewports retain scrolling menus", () => {
  assert.match(app, /class="page-surface" :data-page="activeWorkspaceKey"/);
  const control = readFileSync(
    new URL("./src/components/pages/ControlPage.vue", import.meta.url),
    "utf8",
  );
  assert.match(
    control,
    /<style scoped>[\s\S]*\.mn-control/,
    "run-page styling stays local to its component",
  );
  assert.match(
    css,
    /\.mn-utility-sheet\s*\{[^}]*max-height:[^}]*100dvh[^}]*overflow-y: auto/s,
  );
});

test("workspace navigation selects the requested workspace and resets settings detail", () => {
  const warmWorkspaceCalls = [];
  const loadedWorkspaces = [];
  let written = 0;
  const scope = {
    activeWorkspaceKey: { value: "settings" },
    settingsRoute: { value: "outbound" },
    warmWorkspace: (workspace) => warmWorkspaceCalls.push(workspace),
    workspaceLoaders: {
      dashboard: () => { loadedWorkspaces.push("dashboard"); },
      nodes: () => { loadedWorkspaces.push("nodes"); },
      subs: () => { loadedWorkspaces.push("subs"); },
      settings: () => { loadedWorkspaces.push("settings"); },
    },
    writeLocation: () => { written += 1; },
  };
  const source = app
    .match(
      /function selectWorkspace\(workspace: WorkspaceKey\): void \{[\s\S]*?\n\}/,
    )[0]
    .replace(": WorkspaceKey", "")
    .replace(": void", "");
  runInNewContext(source, scope);
  scope.selectWorkspace("subs");
  assert.equal(scope.activeWorkspaceKey.value, "subs");
  assert.equal(scope.settingsRoute.value, null);
  assert.deepEqual(warmWorkspaceCalls, ["subs"]);
  assert.deepEqual(loadedWorkspaces, ["subs"]);
  assert.equal(written, 1);
});
