import { t } from "@/i18n";

export type RoutingPolicyRule = { conditions: Record<string, unknown>; target: string; available?: boolean };
export type RoutingPolicyGroup = { tag: string; type: "selector" | "urltest"; members: string[]; default?: string; url?: string };
export type RoutingPolicy = { schema: 1; rules: RoutingPolicyRule[]; groups: RoutingPolicyGroup[]; final: string };
export type RoutingPolicySnapshot = RoutingPolicy & {
  revision: string;
  source: string;
  targets: { tag: string; type: string }[];
  ruleSets: string[];
  protectedCount: number;
};

const matchKeys = new Set([
  "domain", "domain_suffix", "domain_keyword", "domain_regex", "ip_cidr", "source_ip_cidr",
  "port", "port_range", "source_port", "source_port_range", "protocol", "network", "process_name",
  "process_path", "package_name", "rule_set", "ip_version", "ip_is_private", "source_ip_is_private",
  "invert", "type", "mode", "rules",
]);

function object(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function supportsRoutingPolicy(text: string): boolean {
  try {
    const root = JSON.parse(text);
    return root.schema === 1 && root.ok === true && root.command === "machine.capabilities"
      && Array.isArray(root.data?.commands) && root.data.commands.includes("routing.inspect")
      && Array.isArray(root.data?.private_commands) && root.data.private_commands.includes("routing.inspect")
      && Array.isArray(root.data?.features) && root.data.features.includes("routing_policy_management");
  } catch { return false; }
}

export function parseRoutingPolicySnapshot(text: string): RoutingPolicySnapshot | null {
  try {
    const envelope = JSON.parse(text);
    if (envelope.schema !== 1 || envelope.ok !== true || envelope.command !== "routing.inspect") return null;
    const policy = envelope.data?.policy;
    if (!object(policy) || policy.schema !== 1 || typeof policy.revision !== "string"
      || !/^[a-f0-9]{64}$/.test(policy.revision) || typeof policy.final !== "string"
      || !Array.isArray(policy.rules) || policy.rules.length > 256 || !Array.isArray(policy.groups)
      || policy.groups.length > 64 || !Array.isArray(policy.targets) || !Array.isArray(policy.rule_sets)) return null;
    if (!policy.rules.every((rule) => object(rule) && object(rule.conditions) && typeof rule.target === "string")
      || !policy.groups.every((group) => object(group) && typeof group.tag === "string"
        && ["selector", "urltest"].includes(String(group.type)) && Array.isArray(group.members)
        && group.members.every((member: unknown) => typeof member === "string"))) return null;
    return {
      schema: 1, revision: policy.revision, source: typeof policy.source === "string" ? policy.source : "defaults",
      rules: policy.rules as RoutingPolicyRule[], groups: policy.groups as RoutingPolicyGroup[], final: policy.final,
      targets: policy.targets.filter((target): target is { tag: string; type: string } => object(target)
        && typeof target.tag === "string" && typeof target.type === "string"),
      ruleSets: policy.rule_sets.filter((tag): tag is string => typeof tag === "string"),
      protectedCount: typeof policy.protected_count === "number" ? policy.protected_count : 0,
    };
  } catch { return null; }
}

export function copyRoutingPolicy(snapshot: RoutingPolicy): RoutingPolicy {
  return {
    schema: 1,
    rules: snapshot.rules.map((rule) => ({ conditions: JSON.parse(JSON.stringify(rule.conditions)), target: rule.target })),
    groups: JSON.parse(JSON.stringify(snapshot.groups)), final: snapshot.final,
  };
}

export function validateRoutingConditions(value: unknown, ruleSets: string[], depth = 0): asserts value is Record<string, unknown> {
  if (!object(value) || !Object.keys(value).length || depth > 4 || Object.keys(value).some((key) => !matchKeys.has(key))) {
    throw new Error(t("请填写受支持的非空匹配条件；系统动作和 UID 由模块维护。"));
  }
  if (value.type === "logical") {
    if (!["and", "or"].includes(String(value.mode)) || !Array.isArray(value.rules)
      || !value.rules.length || value.rules.length > 32
      || Object.keys(value).some((key) => !["type", "mode", "rules", "invert"].includes(key))) {
      throw new Error(t("逻辑规则需要 and 或 or，以及非空子规则。"));
    }
    if (value.invert !== undefined && typeof value.invert !== "boolean") throw new Error(t("匹配标志必须为布尔值。"));
    value.rules.forEach((child) => validateRoutingConditions(child, ruleSets, depth + 1));
    return;
  }
  if (Object.keys(value).some((key) => ["type", "mode", "rules"].includes(key))
    || Object.keys(value).every((key) => key === "invert")) throw new Error(t("规则必须包含匹配条件。"));
  for (const [key, entry] of Object.entries(value)) {
    if (["invert", "ip_is_private", "source_ip_is_private"].includes(key)) {
      if (typeof entry !== "boolean") throw new Error(t("匹配标志必须为布尔值。"));
      continue;
    }
    const values = Array.isArray(entry) ? entry : [entry];
    if (!values.length || values.length > 256 || values.some((item) => typeof item === "string"
      ? !item.length || new TextEncoder().encode(item).length > 256 || /[\u0000-\u001f\u007f]/.test(item)
      : !(typeof item === "number" && Number.isSafeInteger(item) && item >= 0 && ["port", "source_port", "ip_version"].includes(key)))) {
      throw new Error(t("匹配值无效或超过数量限制。"));
    }
    if (key === "rule_set" && values.some((tag) => !ruleSets.includes(String(tag)))) throw new Error(t("请选择当前配置中存在的规则集。"));
  }
}

export function routingTargets(snapshot: RoutingPolicySnapshot, policy: RoutingPolicy): { tag: string; type: string }[] {
  const oldGroups = new Set(snapshot.groups.map((group) => group.tag));
  const targets = snapshot.targets.filter((target) => !oldGroups.has(target.tag));
  return [...targets, ...policy.groups.map((group) => ({ tag: group.tag, type: group.type }))];
}

export function buildRoutingPayload(snapshot: RoutingPolicySnapshot, policy: RoutingPolicy) {
  if (policy.rules.length > 256 || policy.groups.length > 64) throw new Error(t("规则或策略组数量超过编辑上限。"));
  const targets = routingTargets(snapshot, policy);
  const known = new Set(targets.map((target) => target.tag));
  const seen = new Set<string>();
  for (const group of policy.groups) {
    if (!group.tag || group.tag.length > 256 || /[\u0000-\u001f\u007f]/.test(group.tag)
      || seen.has(group.tag) || snapshot.targets.some((target) => target.tag === group.tag && !snapshot.groups.some((old) => old.tag === group.tag))) {
      throw new Error(t("策略组名称为空、重复或与现有出站冲突。"));
    }
    seen.add(group.tag);
    if (!group.members.length || group.members.length > 256 || new Set(group.members).size !== group.members.length
      || group.members.some((member) => !known.has(member))) throw new Error(t("策略组必须包含可用且不重复的出站。"));
    if (group.type === "selector" && !group.members.includes(group.default || "")) throw new Error(t("默认出站必须属于本策略组。"));
    if (group.type === "urltest" && (!group.url || !/^https:\/\/[^/@\s]+(?:\/[^\s]*)?$/.test(group.url)
      || group.members.some((member) => !snapshot.targets.some((target) => target.tag === member && !["selector", "urltest", "direct", "block"].includes(target.type))))) {
      throw new Error(t("自动策略组需要 HTTPS 测速地址和具体代理节点。"));
    }
  }
  const graph = new Map(policy.groups.map((group) => [group.tag, group.members]));
  graph.set("final", [policy.final]);
  const visit = (name: string, active: Set<string>) => {
    if (active.has(name)) throw new Error(t("策略组不能循环引用。"));
    const next = new Set(active).add(name);
    for (const member of graph.get(name) || []) visit(member, next);
  };
  for (const name of graph.keys()) visit(name, new Set());
  if (policy.final === "final" || !known.has(policy.final)) throw new Error(t("请选择可用的兜底出站。"));
  for (const rule of policy.rules) {
    validateRoutingConditions(rule.conditions, snapshot.ruleSets);
    if (!known.has(rule.target)) throw new Error(t("规则仍在引用已删除或不可用的出站，请先修改引用。"));
  }
  return { ...copyRoutingPolicy(policy), revision: snapshot.revision };
}

export function renameRoutingGroup(policy: RoutingPolicy, oldName: string, newName: string): void {
  for (const rule of policy.rules) if (rule.target === oldName) rule.target = newName;
  for (const group of policy.groups) {
    group.members = group.members.map((member) => member === oldName ? newName : member);
    if (group.default === oldName) group.default = newName;
  }
  if (policy.final === oldName) policy.final = newName;
}

export function routingGroupReferenced(policy: RoutingPolicy, name: string): boolean {
  return policy.final === name || policy.rules.some((rule) => rule.target === name)
    || policy.groups.some((group) => group.tag !== name && group.members.includes(name));
}
