import { t } from "@/i18n";
import { fnv32Hex } from "@/lib/fnv32";

export type AppPolicyMode = "blacklist" | "whitelist";

export type AppPolicyInsight = {
  label: string;
  value: string;
  tone: "success" | "warning" | "danger" | "neutral";
};

export type AppPolicySummary = {
  summary: string;
  items: AppPolicyInsight[];
  conflicts: string[];
  installedProxy: string[];
  installedDirect: string[];
};

export type AppPolicySafeReportInput = {
  mode: AppPolicyMode;
  proxy: string[];
  direct: string[];
  summary: AppPolicySummary;
};

export function isValidPackageName(pkg: string): boolean {
  return /^[a-zA-Z][a-zA-Z0-9_]*(\.[a-zA-Z][a-zA-Z0-9_]*)+$/.test(pkg);
}

export function buildAppPolicySummary(
  mode: AppPolicyMode,
  proxy: string[],
  direct: string[],
  installedPackages: Set<string>
): AppPolicySummary {
  const directSet = new Set(direct);
  const conflicts = Array.from(new Set([
    ...proxy.filter((pkg) => directSet.has(pkg)),
  ]));
  const installedProxy = installedPackages.size ? proxy.filter((pkg) => installedPackages.has(pkg)) : [];
  const installedDirect = installedPackages.size ? direct.filter((pkg) => installedPackages.has(pkg)) : [];
  const installedKnown = installedPackages.size > 0;
  const unlisted = mode === "whitelist" ? t('绕过当前数据面') : t('进入当前数据面');
  return {
    summary: mode === "whitelist"
      ? t('Proxy 强制代理；Direct 在 MagicNet 内强制直连；未列出应用绕过当前数据面。')
      : t('Proxy 强制代理；Direct 在 MagicNet 内强制直连；未列出应用进入当前数据面。'),
    conflicts,
    installedProxy,
    installedDirect,
    items: [
      insight(t('Proxy 强制'), t('{count} 个', { count: proxy.length }), proxy.length ? "success" : "neutral"),
      insight(t('Direct 直连'), t('{count} 个', { count: direct.length }), direct.length ? "success" : "neutral"),
      insight(t('未列出应用'), unlisted, mode === "blacklist" ? "success" : "neutral"),
      insight(t('名单冲突'), conflicts.length ? t('{count} 个', { count: conflicts.length }) : t('无'), conflicts.length ? "danger" : "success"),
      insight(t('当前列表命中'), installedKnown ? `P ${installedProxy.length} / D ${installedDirect.length}` : t('未读取应用'), installedKnown ? "success" : "warning")
    ]
  };
}

export function formatAppPolicySafeReport(input: AppPolicySafeReportInput): string {
  return [
    "MagicNet app policy",
    "privacy_note=package names omitted; fingerprints are weak change markers, not privacy proof",
    `mode=${input.mode}`,
    `proxy_count=${input.proxy.length}`,
    `direct_count=${input.direct.length}`,
    `summary=${input.summary.summary}`,
    `conflict_count=${input.summary.conflicts.length}`,
    `current_list_proxy=${input.summary.installedProxy.length}`,
    `current_list_direct=${input.summary.installedDirect.length}`,
    `proxy_fingerprint=${fingerprintList(input.proxy)}`,
    `direct_fingerprint=${fingerprintList(input.direct)}`,
      `conflict_fingerprint=${fingerprintList(input.summary.conflicts)}`,
    "",
    "[insights]",
    ...input.summary.items.map((item) => `${item.label}=${item.value} (${item.tone})`)
  ].join("\n").trim();
}

export function formatAppPolicyFullReport(input: AppPolicySafeReportInput): string {
  return [
    "MagicNet app policy",
    "privacy_note=contains package names from app policy lists",
    `mode=${input.mode}`,
    `proxy_count=${input.proxy.length}`,
    `direct_count=${input.direct.length}`,
    `summary=${input.summary.summary}`,
    `conflict_count=${input.summary.conflicts.length}`,
    `current_list_proxy=${input.summary.installedProxy.length}`,
    `current_list_direct=${input.summary.installedDirect.length}`,
    "",
    "[insights]",
    ...input.summary.items.map((item) => `${item.label}=${item.value} (${item.tone})`),
    "",
    "[proxy]",
    ...input.proxy,
    "",
    "[direct]",
    ...input.direct,
    "",
  ].join("\n").trim();
}

function insight(label: string, value: string, tone: AppPolicyInsight["tone"]): AppPolicyInsight {
  return { label, value, tone };
}

function fingerprintList(values: string[]): string {
  if (!values.length) return "none";
  return fnv32Hex(values.slice().sort().join("\n"));
}
