import { t } from "@/i18n";

export const APPLICATION_DNS_TARGET = "__magicnet_dns_profile__";
export const BOOTSTRAP_DNS_TARGET = "bootstrap-local-dns";

export type DnsRuleRow = {
  id: number | null;
  key: string;
  conditions: Record<string, unknown>;
  target: string;
  editable: boolean;
  protectedReason: string | null;
};

export type DnsRulesSnapshot = {
  revision: string;
  source: "user_intent" | "configuration";
  rules: DnsRuleRow[];
  resolvers: Array<{ tag: string; kind: string; available: boolean }>;
  ruleSets: string[];
  finalServer: string | null;
  nodeResolver: string | null;
  applicationServer: string;
  truncated: boolean;
};

const conditionKeys = new Set([
  "domain", "domain_suffix", "domain_keyword", "domain_regex", "rule_set", "query_type",
  "protocol", "network", "inbound", "ip_cidr", "source_ip_cidr", "port", "source_port",
  "ip_is_private", "source_ip_is_private", "invert",
]);

export function supportsDnsInspection(text: string): boolean {
  try {
    const envelope = JSON.parse(text);
    return envelope.schema === 1 && envelope.ok === true && envelope.command === "machine.capabilities"
      && Array.isArray(envelope.data?.commands) && envelope.data.commands.includes("dns.inspect");
  } catch { return false; }
}

export function parseDnsRulesSnapshot(text: string): DnsRulesSnapshot | null {
  try {
    if (text.length > 4 * 1024 * 1024) return null;
    const envelope = JSON.parse(text);
    const data = envelope.data;
    if (envelope.schema !== 1 || envelope.ok !== true || envelope.command !== "dns.inspect"
      || !record(data) || typeof data.revision !== "string" || !/^[a-f0-9]{64}$/.test(data.revision)
      || !["configuration", "user_intent"].includes(data.source as string)
      || typeof data.truncated !== "boolean" || data.runtime_state !== "unknown"
      || !nullableTag(data.final_server) || !nullableTag(data.node_resolver) || !boundedTag(data.application_server)
      || !Array.isArray(data.rules) || data.rules.length > 256
      || !Array.isArray(data.resolvers) || data.resolvers.length > 258
      || !Array.isArray(data.rule_sets) || data.rule_sets.length > 256) return null;
    if (data.rules.some((rule, index) => !record(rule) || rule.id !== index + 1
      || !record(rule.conditions) || (rule.target !== "" && !boundedTag(rule.target)) || typeof rule.editable !== "boolean"
      || (rule.editable ? rule.protected_reason !== null : !["mode", "feature", "unsupported"].includes(rule.protected_reason as string)))) return null;
    if (data.resolvers.some((resolver) => !record(resolver) || !boundedTag(resolver.tag)
      || !["application", "bootstrap", "custom"].includes(resolver.kind as string) || typeof resolver.available !== "boolean")
      || new Set(data.resolvers.map((resolver) => resolver.tag)).size !== data.resolvers.length
      || data.rule_sets.some((tag) => !boundedTag(tag))) return null;
    return {
      revision: data.revision,
      source: data.source === "user_intent" ? "user_intent" : "configuration",
      rules: data.rules.map((rule) => ({
        id: rule.id,
        key: `rule-${rule.id}`,
        conditions: rule.conditions,
        target: rule.target,
        editable: rule.editable,
        protectedReason: typeof rule.protected_reason === "string" ? rule.protected_reason : null,
      })),
      resolvers: data.resolvers,
      ruleSets: data.rule_sets,
      finalServer: typeof data.final_server === "string" ? data.final_server : null,
      nodeResolver: typeof data.node_resolver === "string" ? data.node_resolver : null,
      applicationServer: typeof data.application_server === "string" ? data.application_server : "",
      truncated: data.truncated === true,
    };
  } catch { return null; }
}

export function copyDnsRuleRows(rows: DnsRuleRow[]): DnsRuleRow[] {
  return rows.map((row) => ({ ...row, conditions: JSON.parse(JSON.stringify(row.conditions)) }));
}

export function validateDnsConditions(conditions: unknown, ruleSets: string[]): asserts conditions is Record<string, unknown> {
  if (!record(conditions) || !Object.keys(conditions).length) throw new Error(t("匹配条件必须是非空 JSON 对象。"));
  for (const [key, value] of Object.entries(conditions)) {
    if (!conditionKeys.has(key)) throw new Error(t("匹配条件包含不支持的字段；解析目标请使用独立选择框。"));
    if (["invert", "ip_is_private", "source_ip_is_private"].includes(key)) {
      if (typeof value !== "boolean") throw new Error(t("布尔匹配条件必须使用 true 或 false。"));
      continue;
    }
    const values = Array.isArray(value) ? value : [value];
    if (!values.length || values.length > 128) throw new Error(t("每个匹配条件需要 1–128 个值。"));
    for (const member of values) {
      if (["port", "source_port", "query_type"].includes(key) && typeof member === "number") {
        if (!Number.isInteger(member) || member > 65535 || member < (key === "query_type" ? 0 : 1)) {
          throw new Error(t("端口或查询类型数值超出允许范围。"));
        }
      } else if (typeof member !== "string" || !member.length || new TextEncoder().encode(member).length > 256 || /[\u0000-\u001f\u007f-\u009f]/.test(member)) {
        throw new Error(t("匹配值必须是有界字符串，不能包含控制字符。"));
      }
      if (key === "rule_set" && (typeof member !== "string" || !ruleSets.includes(member))) {
        throw new Error(t("规则引用了当前配置中不存在的 rule-set。"));
      }
    }
  }
}

export function buildDnsRulesPayload(snapshot: DnsRulesSnapshot, rows: DnsRuleRow[]) {
  if (snapshot.truncated || rows.length > 256) throw new Error(t("规则快照超过编辑上限，不能从截断数据保存。"));
  const originals = new Map(snapshot.rules.map((row) => [row.id, row]));
  const seen = new Set<number>();
  const rules = rows.map((row, index) => {
    const original = row.id === null ? null : originals.get(row.id);
    if (row.id !== null) {
      if (!original || seen.has(row.id)) throw new Error(t("规则标识无效或重复，请重新读取规则。"));
      seen.add(row.id);
    }
    if (original && !original.editable) {
      if (row.id !== index + 1 || row.target !== original.target
        || JSON.stringify(row.conditions) !== JSON.stringify(original.conditions)) {
        throw new Error(t("受保护规则不能修改、删除或移动。"));
      }
      return { id: row.id };
    }
    validateDnsConditions(row.conditions, snapshot.ruleSets);
    if (!snapshot.resolvers.some((resolver) => resolver.tag === row.target && resolver.available)) {
      throw new Error(t("请选择当前配置中可用的 DNS 解析目标。"));
    }
    return { id: row.id, conditions: row.conditions, target: row.target };
  });
  if (snapshot.rules.some((row) => !row.editable && row.id !== null && !seen.has(row.id))) {
    throw new Error(t("受保护规则不能修改、删除或移动。"));
  }
  return { schema: 1, revision: snapshot.revision, rules };
}

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function boundedTag(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && new TextEncoder().encode(value).length <= 256 && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
}

function nullableTag(value: unknown): boolean {
  return value === null || boundedTag(value);
}
