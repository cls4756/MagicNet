import assert from "node:assert/strict";
import test from "node:test";
import {
  appPolicyListDeltas,
  appPolicyPendingCount,
  appPolicySyncPayload,
  isAppPolicyTargetSelected,
  normalizeAppPolicyLists,
  sameAppPolicyLists,
  toggleAppPolicyTarget,
} from "./src/components/pages/appPolicyDraft.ts";

const applied = normalizeAppPolicyLists(["com.a", "com.b"], ["com.c"]);

test("toggling a checkbox only edits the draft", () => {
  const draft = toggleAppPolicyTarget(applied, "proxy", "com.d");
  assert.deepEqual(applied.proxy, ["com.a", "com.b"]);
  assert.deepEqual(applied.direct, ["com.c"]);
  assert.deepEqual(draft.proxy, ["com.a", "com.b", "com.d"]);
  assert.equal(isAppPolicyTargetSelected(applied, "proxy", "com.d"), false);
  assert.equal(isAppPolicyTargetSelected(draft, "proxy", "com.d"), true);
});

test("unchecking removes an entry and re-checking appends it", () => {
  const removed = toggleAppPolicyTarget(applied, "proxy", "com.a");
  assert.deepEqual(removed.proxy, ["com.b"]);
  const readded = toggleAppPolicyTarget(removed, "proxy", "com.a");
  assert.deepEqual(readded.proxy, ["com.b", "com.a"]);
  assert.ok(sameAppPolicyLists(readded, applied), "re-adding restores the applied members");
});

test("a package moves out of the other list", () => {
  const draft = toggleAppPolicyTarget(applied, "proxy", "com.c");
  assert.deepEqual(draft.proxy, ["com.a", "com.b", "com.c"]);
  assert.deepEqual(draft.direct, []);
});

test("deltas report adds and removes per list", () => {
  const draft = { proxy: ["com.b", "com.d"], direct: ["com.e"] };
  assert.deepEqual(appPolicyListDeltas(applied, draft), [
    { target: "proxy", added: ["com.d"], removed: ["com.a"] },
    { target: "direct", added: ["com.e"], removed: ["com.c"] },
  ]);
  assert.equal(appPolicyPendingCount(applied, draft), 4);
});

test("applying the applied state again is not a pending change", () => {
  assert.ok(sameAppPolicyLists(applied, normalizeAppPolicyLists(["com.b", "com.a"], ["com.c"])));
  assert.equal(appPolicyPendingCount(applied, applied), 0);
});

test("the sync payload carries only the packages the user touched", () => {
  const deltas = appPolicyListDeltas(applied, {
    proxy: ["com.b", "com.d"],
    direct: ["com.e"],
  });
  assert.equal(
    appPolicySyncPayload(deltas),
    "add proxy com.d\nremove proxy com.a\nadd direct com.e\nremove direct com.c",
  );
  assert.equal(appPolicySyncPayload([]), "");
});

console.log("app policy draft tests passed");
