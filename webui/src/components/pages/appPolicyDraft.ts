/**
 * Draft editing for the proxy/direct app lists.
 *
 * Toggling a checkbox never writes to the device. The page keeps the complete
 * desired state in memory and only sends it through `app sync` when the user
 * presses apply, so a multi-app edit costs one core restart instead of one
 * restart per row. Until then the rendered order still follows the applied
 * state, which keeps checked rows from jumping to the top mid-edit.
 */

export type AppPolicyTarget = "proxy" | "direct";

export type AppPolicyLists = {
  proxy: string[];
  direct: string[];
};

export type AppPolicyListDelta = {
  target: AppPolicyTarget;
  added: string[];
  removed: string[];
};

export const APP_POLICY_TARGETS: readonly AppPolicyTarget[] = ["proxy", "direct"];

export function normalizeAppPolicyLists(
  proxy: readonly string[],
  direct: readonly string[],
): AppPolicyLists {
  return { proxy: uniqueMembers(proxy), direct: uniqueMembers(direct) };
}

export function isAppPolicyTargetSelected(
  lists: AppPolicyLists,
  target: AppPolicyTarget,
  packageName: string,
): boolean {
  return lists[target].includes(packageName);
}

/** Proxy and direct are mutually exclusive, so the other list drops the package. */
export function toggleAppPolicyTarget(
  lists: AppPolicyLists,
  target: AppPolicyTarget,
  packageName: string,
): AppPolicyLists {
  const next: AppPolicyLists = { proxy: [...lists.proxy], direct: [...lists.direct] };
  const index = next[target].indexOf(packageName);
  if (index >= 0) next[target].splice(index, 1);
  else next[target].push(packageName);
  const other = otherTarget(target);
  next[other] = next[other].filter((item) => item !== packageName);
  return next;
}

export function sameAppPolicyLists(a: AppPolicyLists, b: AppPolicyLists): boolean {
  return sameMembers(a.proxy, b.proxy) && sameMembers(a.direct, b.direct);
}

export function appPolicyListDeltas(
  applied: AppPolicyLists,
  draft: AppPolicyLists,
): AppPolicyListDelta[] {
  const deltas: AppPolicyListDelta[] = [];
  for (const target of APP_POLICY_TARGETS) {
    const appliedSet = new Set(applied[target]);
    const draftSet = new Set(draft[target]);
    const added = draft[target].filter((item) => !appliedSet.has(item));
    const removed = applied[target].filter((item) => !draftSet.has(item));
    if (added.length || removed.length) deltas.push({ target, added, removed });
  }
  return deltas;
}

export function appPolicyPendingCount(applied: AppPolicyLists, draft: AppPolicyLists): number {
  return appPolicyListDeltas(applied, draft).reduce(
    (total, delta) => total + delta.added.length + delta.removed.length,
    0,
  );
}

/**
 * `cli app sync` reads one `<add|remove> <proxy|direct> <package>` line per
 * change. Only packages the user actually touched are sent, so a stale or
 * partially loaded page can never drop list entries it never displayed.
 */
export function appPolicySyncPayload(deltas: readonly AppPolicyListDelta[]): string {
  return deltas.flatMap(({ target, added, removed }) => [
    ...added.map((item) => `add ${target} ${item}`),
    ...removed.map((item) => `remove ${target} ${item}`),
  ]).join("\n");
}

function otherTarget(target: AppPolicyTarget): AppPolicyTarget {
  return target === "proxy" ? "direct" : "proxy";
}

function uniqueMembers(values: readonly string[]): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const value of values) {
    const item = value.trim();
    if (!item || seen.has(item)) continue;
    seen.add(item);
    result.push(item);
  }
  return result;
}

function sameMembers(a: readonly string[], b: readonly string[]): boolean {
  if (a.length !== b.length) return false;
  const members = new Set(b);
  return a.every((item) => members.has(item));
}
