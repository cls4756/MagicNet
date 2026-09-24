export type RoutingRuleSummary = {
  order: number;
  conditions: Record<string, unknown>;
  outbound: string | null;
  source: string;
  managedDomains: string[];
  managedTarget: string | null;
};

export type RoutingReference = {
  kind: "route_rule" | "outbound_member" | "route_final";
  order?: number;
  tag?: string;
};

export type RoutingOutboundSummary = {
  tag: string;
  type: string;
  source: string;
  internal: boolean;
  members: string[];
  default: string | null;
  selected: string | null;
  references: RoutingReference[];
  referenceCount: number;
};

export type RoutingInspectSnapshot = {
  rules: RoutingRuleSummary[];
  outbounds: RoutingOutboundSummary[];
  finalOutbound: string | null;
  runtimeState: "observed" | "unknown";
  truncated: boolean;
};

export function parseRoutingInspectSnapshot(text: string): RoutingInspectSnapshot | null {
  try {
    const envelope = objectValue(JSON.parse(text));
    if (envelope?.schema !== 1 || envelope.ok !== true || envelope.command !== "routing.inspect") return null;
    const data = objectValue(envelope.data);
    if (!data || !Array.isArray(data.rules) || !Array.isArray(data.outbounds)) return null;
    return {
      rules: data.rules.map(parseRule).filter((rule): rule is RoutingRuleSummary => Boolean(rule)),
      outbounds: data.outbounds.map(parseOutbound).filter((outbound): outbound is RoutingOutboundSummary => Boolean(outbound)),
      finalOutbound: stringOrNull(data.final_outbound),
      runtimeState: data.runtime_state === "observed" ? "observed" : "unknown",
      truncated: data.truncated === true,
    };
  } catch {
    return null;
  }
}

function parseRule(value: unknown): RoutingRuleSummary | null {
  const rule = objectValue(value);
  if (!rule || typeof rule.order !== "number") return null;
  return {
    order: rule.order,
    conditions: objectValue(rule.conditions) || {},
    outbound: stringOrNull(rule.outbound),
    source: typeof rule.source === "string" ? rule.source : "effective_config",
    managedDomains: stringArray(rule.managed_domains),
    managedTarget: stringOrNull(rule.managed_target),
  };
}

function parseOutbound(value: unknown): RoutingOutboundSummary | null {
  const outbound = objectValue(value);
  if (!outbound || typeof outbound.tag !== "string") return null;
  const references = Array.isArray(outbound.references)
    ? outbound.references.map(parseReference).filter((reference): reference is RoutingReference => Boolean(reference))
    : [];
  return {
    tag: outbound.tag,
    type: typeof outbound.type === "string" ? outbound.type : "unknown",
    source: typeof outbound.source === "string" ? outbound.source : "configured",
    internal: outbound.internal === true,
    members: stringArray(outbound.members),
    default: stringOrNull(outbound.default),
    selected: stringOrNull(outbound.selected),
    references,
    referenceCount: typeof outbound.reference_count === "number" ? outbound.reference_count : references.length,
  };
}

function parseReference(value: unknown): RoutingReference | null {
  const reference = objectValue(value);
  if (!reference || !["route_rule", "outbound_member", "route_final"].includes(String(reference.kind))) return null;
  return {
    kind: reference.kind as RoutingReference["kind"],
    order: typeof reference.order === "number" ? reference.order : undefined,
    tag: typeof reference.tag === "string" ? reference.tag : undefined,
  };
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
}

function stringOrNull(value: unknown): string | null {
  return typeof value === "string" && value ? value : null;
}
