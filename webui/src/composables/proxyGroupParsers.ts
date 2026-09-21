export type ProxyGroupSummary = {
  name: string;
  type: string;
  now: string;
  proxies: string[];
  kind: "selector" | "auto" | "provider";
  selectable: boolean;
};

export type ProxyNodeSummary = {
  name: string;
  type: string;
};

export { sanitizeNodeText as sanitizeProxyName } from "./nodeDelayParsers.ts";

export type ProxyGroupsSnapshot = {
  groups: ProxyGroupSummary[];
  nodes: ProxyNodeSummary[];
};

export function parseProxyGroupsSnapshot(text: string): ProxyGroupsSnapshot | null {
  try {
    const root = objectValue(JSON.parse(text));
    if (!root) return null;
    const proxies = objectValue(root.proxies);
    const providers = objectValue(root.providers);
    const groups = [
      ...parseProxyGroupObject(proxies, "selector"),
      ...parseProxyGroupObject(providers, "provider")
    ];
    if (!groups.length) groups.push(...parseProxyGroupObject(root, "provider"));
    return {
      groups: Array.from(new Map(groups.map((group) => [group.name, group])).values())
        .sort((left, right) => right.proxies.length - left.proxies.length),
      nodes: parseProxyNodeObject(proxies),
    };
  } catch {
    return null;
  }
}

function parseProxyGroupObject(
  source: Record<string, unknown> | null,
  sourceKind: "selector" | "provider",
): ProxyGroupSummary[] {
  if (!source) return [];
  return Object.entries(source)
    .map(([key, value]) => parseProxyGroup(key, objectValue(value), sourceKind))
    .filter((group): group is ProxyGroupSummary => Boolean(group));
}

function parseProxyGroup(
  key: string,
  item: Record<string, unknown> | null,
  sourceKind: "selector" | "provider",
): ProxyGroupSummary | null {
  if (!item) return null;
  const type = stringValue(item.type) || "provider";
  const kind = sourceKind === "provider" ? "provider" : proxyGroupKind(type);
  if (sourceKind === "selector" && !isProxyGroupType(type)) return null;
  // /proxies selectors use `all`; provider responses use `proxies`.
  const members = Array.isArray(item.all) ? item.all : item.proxies;
  const proxies = Array.isArray(members)
    ? members.map(proxyName).filter((name): name is string => Boolean(name))
    : [];
  if (!proxies.length && !stringValue(item.now)) return null;
  return {
    name: stringValue(item.name) || key,
    type,
    now: stringValue(item.now),
    proxies,
    kind,
    selectable: kind === "selector",
  };
}

function parseProxyNodeObject(source: Record<string, unknown> | null): ProxyNodeSummary[] {
  if (!source) return [];
  return Object.entries(source)
    .map(([key, value]) => {
      const item = objectValue(value);
      if (!item) return null;
      const type = stringValue(item.type);
      if (!type || isProxyGroupType(type) || isBuiltinProxy(key, type)) return null;
      return { name: stringValue(item.tag) || key, type };
    })
    .filter((node): node is ProxyNodeSummary => Boolean(node))
    .sort((left, right) => left.name.localeCompare(right.name));
}

function isProxyGroupType(type: string): boolean {
  return /^(selector|urltest|url-test|fallback|loadbalance|least-ping|random)$/i.test(type.trim());
}

function proxyGroupKind(type: string): "selector" | "auto" {
  return /^(urltest|url-test|fallback|loadbalance|least-ping|random)$/i.test(type.trim())
    ? "auto"
    : "selector";
}

function isBuiltinProxy(name: string, type: string): boolean {
  return new Set(["direct", "block", "dns", "dns-guard", "tun", "mixed"]).has(name.toLowerCase())
    || /^(direct|block|dns|tun)$/i.test(type.trim());
}

function proxyName(value: unknown): string | null {
  if (typeof value === "string") return value;
  const object = objectValue(value);
  return object ? stringValue(object.name) || null : null;
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

function stringValue(value: unknown): string {
  return typeof value === "string" ? value : "";
}
