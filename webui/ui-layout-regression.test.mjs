import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const app = readFileSync(new URL("./src/App.vue", import.meta.url), "utf8");
const apps = readFileSync(new URL("./src/components/pages/AppsPage.vue", import.meta.url), "utf8");
const styles = readFileSync(new URL("./src/styles.css", import.meta.url), "utf8");
const button = readFileSync(new URL("./src/components/ui/Button.vue", import.meta.url), "utf8");

assert.match(app, /type WorkspaceKey = "dashboard" \| "nodes" \| "subs" \| "settings"/);
assert.match(styles, /\.mobile-nav\s*\{[\s\S]*grid-template-columns:\s*repeat\(4,/);
assert.match(styles, /\.mobile-nav button span\s*\{[\s\S]*overflow-wrap:\s*anywhere[\s\S]*white-space:\s*normal/);
assert.match(styles, /\.mn-section-tabs\s*\{[\s\S]*overflow-x:\s*auto/);
assert.match(styles, /\.desktop-rail nav button\s*\{[\s\S]*grid-template-columns:\s*auto minmax\(0, 1fr\)/);
assert.match(styles, /\.mn-shell\s*\{[\s\S]*padding:[^;]*env\(safe-area-inset-bottom\)/);
assert.match(button, /border text-sm font-medium/, "shared buttons must own their readable text size after font inheritance");
assert.match(apps, /min-h-12 whitespace-nowrap[\s\S]*全局接管/);
assert.match(apps, /min-h-12 whitespace-nowrap[\s\S]*仅名单接管/);
assert.match(apps, /<Card class="mn-section-lead grid gap-3">/, "the page opens without a divider above the first section");
assert.match(apps, /<h2 class="sr-only">[\s\S]*t\('应用名单'\)/, "the page keeps its title for assistive tech");
assert.doesNotMatch(
  apps,
  /<PageHeader/,
  "the section tabs already name this page, so it must not repeat a page header",
);
assert.match(styles, /\.magic-card\.mn-section-lead\s*\{\s*border-top:\s*1px solid var\(--mn-border\);/);
assert.equal(apps.match(/<ScrollBox /g)?.length, 2, "both app lists need the visible scroll rail");
assert.equal(apps.match(/<\/ScrollBox>/g)?.length, 2, "every ScrollBox must be closed again");
assert.match(styles, /\.mn-scroll-box__thumb\s*\{[\s\S]*?background-color:/);

console.log("UI layout regression tests passed");
