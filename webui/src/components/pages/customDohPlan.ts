import { t } from "@/i18n";

export type CustomDohSnapshot = {
  configured: boolean;
  url: string;
  viaProxy: boolean;
  revision: string;
};

export function supportsCustomDoh(text: string): boolean {
  try {
    const envelope = JSON.parse(text);
    return envelope.schema === 1 && envelope.ok === true && envelope.command === "machine.capabilities"
      && Array.isArray(envelope.data?.commands) && envelope.data.commands.includes("dns.custom.inspect")
      && Array.isArray(envelope.data?.features) && envelope.data.features.includes("custom_doh")
      && Array.isArray(envelope.data?.private_commands) && envelope.data.private_commands.includes("dns.custom.inspect");
  } catch { return false; }
}

export function validateCustomDohUrl(value: string): string {
  const url = value.trim();
  const invalid = () => new Error(t("请输入有效的 HTTPS DoH 地址，不支持用户名、密码或片段。"));
  if (!url.startsWith("https://") || url.length > 4096 || /[^\x21-\x7e]|[\\"<>`{}|^#]/.test(url)
    || /%(?![0-9a-f]{2})|%(?:0[0-9a-f]|1[0-9a-f]|7f)/i.test(url)) throw invalid();
  try {
    const authority = url.slice(8).split(/[/?]/)[0]!;
    if (authority.includes("@") || authority.endsWith(":")) throw invalid();
    const parsed = new URL(url);
    if (parsed.username || parsed.password || parsed.hash || !parsed.hostname || parsed.port === "0") throw invalid();
    const host = parsed.hostname;
    if (!host.startsWith("[") && (host.length > 253 || !host.replace(/\.$/, "").split(".").every((label) =>
      /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/i.test(label)))) throw invalid();
    const rawHost = authority.startsWith("[") ? authority.slice(0, authority.indexOf("]") + 1) : authority.split(":")[0]!;
    if (/^[0-9.]+$/.test(rawHost) && rawHost !== host) throw invalid();
    const suffix = url.slice(8 + authority.length);
    if ((suffix.startsWith("?") ? `/dns-query${suffix}` : suffix || "/dns-query").length > 2048) throw invalid();
  } catch { throw invalid(); }
  return url;
}

export function parseCustomDohSnapshot(text: string): CustomDohSnapshot | null {
  try {
    if (text.length > 16384) return null;
    const envelope = JSON.parse(text);
    const data = envelope.data;
    if (envelope.schema !== 1 || envelope.ok !== true || envelope.command !== "dns.custom.inspect"
      || !data || typeof data.configured !== "boolean" || typeof data.via_proxy !== "boolean"
      || typeof data.revision !== "string" || !/^[a-f0-9]{64}$/.test(data.revision)
      || (data.configured ? typeof data.url !== "string" : data.url !== null)) return null;
    const url = data.configured ? validateCustomDohUrl(data.url) : "";
    return { configured: data.configured, url, viaProxy: data.via_proxy, revision: data.revision };
  } catch { return null; }
}

export function buildCustomDohPayload(snapshot: CustomDohSnapshot, url: string, viaProxy: boolean) {
  if (!/^[a-f0-9]{64}$/.test(snapshot.revision) || typeof viaProxy !== "boolean") {
    throw new Error(t("请重新读取自定义 DoH 配置后再保存。"));
  }
  return { schema: 1, revision: snapshot.revision, url: validateCustomDohUrl(url), via_proxy: viaProxy };
}
