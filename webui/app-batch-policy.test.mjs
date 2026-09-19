import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { runInNewContext } from "node:vm";
import ts from "typescript";

const page = readFileSync(
  new URL("./src/components/pages/AppsPage.vue", import.meta.url),
  "utf8",
);

// The apps page uses per-app checkboxes in searchable list boxes instead of
// a batch-select + bulk-apply panel. Each checkbox toggles membership via
// toggleAppList, which calls addPackage or removeApp directly.
assert.match(page, /type="checkbox"[\s\S]*toggleAppList/);
assert.doesNotMatch(page, /function selectVisiblePackages/);
assert.doesNotMatch(page, /function requestBatchAdd/);
assert.doesNotMatch(page, /applyBatchAdd/);
assert.doesNotMatch(page, /togglePackageSelection/);

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
