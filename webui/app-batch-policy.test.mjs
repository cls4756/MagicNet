import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { runInNewContext } from "node:vm";
import ts from "typescript";

const page = readFileSync(
  new URL("./src/components/pages/AppsPage.vue", import.meta.url),
  "utf8",
);

// A checkbox only edits a local draft. The proxy and direct lists are written
// once per apply, through a single `app sync` call, so a multi-row edit costs
// one core restart instead of one restart per toggled row.
assert.match(page, /type="checkbox"[\s\S]*toggleApp\(/);
assert.match(page, /function applyPendingLists/);
assert.match(page, /`app sync \$\{shellQuote\(payload\)\}`/);
assert.doesNotMatch(page, /app add /);
assert.doesNotMatch(page, /app remove /);
assert.doesNotMatch(page, /requestRemoveApp/);
assert.doesNotMatch(page, /勾选加入/);

// The list boxes must actually read as lists: bounded height, scrolling, border.
assert.match(
  page,
  /max-h-80 overflow-y-auto overscroll-contain rounded-\[var\(--mn-radius-md\)\] border border-\[var\(--mn-border\)\]/,
);
assert.match(page, /<ul class="divide-y/);
assert.match(page, /pendingCount \? `\$\{t\('应用更改'\)\} \(\$\{pendingCount\}\)` : t\('应用更改'\)/);

// Execute the shipped handler, not a reimplementation of its async ownership.
const script = page.match(/<script setup lang="ts">([\s\S]*?)<\/script>/)?.[1];
assert.ok(script);
const ast = ts.createSourceFile(
  "AppsPage.ts",
  script,
  ts.ScriptTarget.Latest,
  true,
);
const handler = ast.statements.find(
  (node) =>
    ts.isFunctionDeclaration(node) && node.name?.text === "confirmAppAction",
);
assert.ok(handler);
const handlerJs = ts.transpileModule(handler.getText(ast), {
  compilerOptions: { target: ts.ScriptTarget.ES2022 },
}).outputText;

for (const fails of [false, true]) {
  test(`an ${fails ? "unsuccessful" : "successful"} app action preserves a newer confirmation`, async () => {
    const task = Promise.withResolvers();
    const pendingAppAction = {
      value: { key: "apply-batch", run: () => task.promise },
    };
    const confirm = runInNewContext(`${handlerJs}\nconfirmAppAction`, {
      pendingAppAction,
    });
    const completion = confirm();
    assert.equal(pendingAppAction.value, null);
    await confirm(); // A second confirmation cannot submit the consumed action.
    const next = { key: "apply-batch", run: async () => {} };
    pendingAppAction.value = next;
    if (fails) {
      const error = new Error("apply failed");
      task.reject(error);
      await assert.rejects(completion, (actual) => actual === error);
    } else {
      task.resolve();
      await completion;
    }
    assert.equal(pendingAppAction.value, next);
  });
}

console.log("app batch policy tests passed");
