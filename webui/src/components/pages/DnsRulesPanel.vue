<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from "vue";
import { ArrowDown, ArrowUp, Pencil, Plus, RefreshCw, Trash2 } from "lucide-vue-next";
import { t } from "@/i18n";
import Button from "@/components/ui/Button.vue";
import Card from "@/components/ui/Card.vue";
import ConfirmPanel from "@/components/ui/ConfirmPanel.vue";
import Textarea from "@/components/ui/Textarea.vue";
import { useActionLock } from "@/composables/useActionLock";
import { useMagicNet } from "@/composables/useMagicNet";
import { useVisibilityTask } from "@/composables/useVisibilityTask";
import { APPLICATION_DNS_TARGET, BOOTSTRAP_DNS_TARGET, buildDnsRulesPayload, copyDnsRuleRows, parseDnsRulesSnapshot, supportsDnsInspection, validateDnsConditions, type DnsRuleRow, type DnsRulesSnapshot } from "./dnsRulesPlan";

const { runCli, runPrivateCli, stagePrivatePayload, removePrivatePayload, shellQuote } = useMagicNet();
const { isRunning, withAction } = useActionLock();
const snapshot = ref<DnsRulesSnapshot | null>(null);
const rows = ref<DnsRuleRow[]>([]);
const draft = ref<{ key: string | null; conditions: string; target: string } | null>(null);
const pendingApply = ref(false);
const error = ref("");
const status = ref("");
let disposed = false;
let readSequence = 0;
let draftSequence = 0;
let capabilityChecked = false;
const busy = computed(() => isRunning("dns-rules-load") || isRunning("dns-rules-save"));
const changed = computed(() => Boolean(snapshot.value) && JSON.stringify(rows.value) !== JSON.stringify(snapshot.value?.rules));

function targetLabel(target: string): string {
  if (target === APPLICATION_DNS_TARGET) return t("应用 DNS Profile");
  if (target === BOOTSTRAP_DNS_TARGET) return "Bootstrap DNS";
  return target || t("未知");
}

function protectedLabel(row: DnsRuleRow): string {
  if (row.protectedReason === "mode") return t("模式规则由模块维护");
  if (row.protectedReason === "feature") return t("功能解析规则由模块维护");
  return t("高级规则只读，原始内容会保留");
}

async function loadSnapshot(force = false): Promise<void> {
  if (!force && (changed.value || draft.value || pendingApply.value)) return;
  const sequence = ++readSequence;
  if (!capabilityChecked) {
    const capabilities = await runCli("--json capabilities", t("读取 DNS 规则能力"), true);
    if (disposed || sequence !== readSequence) return;
    if (!supportsDnsInspection(capabilities)) {
      error.value = t("当前模块不支持 DNS 规则界面，请升级模块。");
      return;
    }
    capabilityChecked = true;
  }
  const outcome = await runPrivateCli("--json dns inspect", t("读取 DNS 规则"), "--json dns inspect");
  if (disposed || sequence !== readSequence || (!force && (changed.value || draft.value))) return;
  const parsed = outcome.ok ? parseDnsRulesSnapshot(outcome.stdout) : null;
  if (!parsed) {
    error.value = t("无法读取 DNS 规则，请检查当前配置。");
    return;
  }
  snapshot.value = parsed;
  rows.value = copyDnsRuleRows(parsed.rules);
  error.value = "";
}

async function refresh(): Promise<void> {
  await withAction("dns-rules-load", () => loadSnapshot());
}

function openDraft(row?: DnsRuleRow): void {
  if (busy.value || snapshot.value?.truncated || (row && !row.editable)) return;
  draft.value = {
    key: row?.key ?? null,
    conditions: JSON.stringify(row?.conditions ?? { domain_suffix: ["example.com"] }, null, 2),
    target: row?.target ?? APPLICATION_DNS_TARGET,
  };
  pendingApply.value = false;
  error.value = "";
}

function saveDraft(): void {
  if (!draft.value || !snapshot.value || busy.value) return;
  try {
    const conditions: unknown = JSON.parse(draft.value.conditions);
    validateDnsConditions(conditions, snapshot.value.ruleSets);
    if (!snapshot.value.resolvers.some((resolver) => resolver.tag === draft.value?.target && resolver.available)) {
      throw new Error(t("请选择当前配置中可用的 DNS 解析目标。"));
    }
    const existing = rows.value.find((row) => row.key === draft.value?.key);
    if (existing) {
      existing.conditions = conditions;
      existing.target = draft.value.target;
    } else {
      rows.value.push({ id: null, key: `new-${++draftSequence}`, conditions, target: draft.value.target, editable: true, protectedReason: null });
    }
    draft.value = null;
    error.value = "";
    status.value = t("规则修改尚未保存，请校验并应用。");
  } catch (failure) {
    error.value = failure instanceof SyntaxError ? t("匹配条件不是有效 JSON。")
      : failure instanceof Error ? failure.message : t("DNS 规则校验失败。");
  }
}

function move(index: number, direction: number): void {
  const destination = index + direction;
  if (busy.value || !rows.value[index]?.editable || !rows.value[destination]?.editable) return;
  const [row] = rows.value.splice(index, 1);
  rows.value.splice(destination, 0, row!);
  pendingApply.value = false;
}

function canRemove(index: number): boolean {
  return rows.value[index]?.editable === true && !rows.value.slice(index + 1).some((row) => !row.editable);
}

function remove(index: number): void {
  if (busy.value || !canRemove(index)) return;
  rows.value.splice(index, 1);
  pendingApply.value = false;
}

function discard(): void {
  if (!snapshot.value || busy.value) return;
  rows.value = copyDnsRuleRows(snapshot.value.rules);
  draft.value = null;
  pendingApply.value = false;
  error.value = "";
  status.value = "";
}

function requestApply(): void {
  if (!snapshot.value || busy.value || draft.value) return;
  try {
    buildDnsRulesPayload(snapshot.value, rows.value);
    pendingApply.value = true;
    error.value = "";
  } catch (failure) {
    error.value = failure instanceof Error ? failure.message : t("DNS 规则校验失败。");
  }
}

async function apply(): Promise<void> {
  if (!snapshot.value || busy.value || !pendingApply.value) return;
  let payload: string;
  try { payload = JSON.stringify(buildDnsRulesPayload(snapshot.value, rows.value)); }
  catch (failure) {
    error.value = failure instanceof Error ? failure.message : t("DNS 规则校验失败。");
    return;
  }
  await withAction("dns-rules-save", async () => {
    const basename = `dns-rules-${Date.now()}-${Math.random().toString(36).slice(2, 8)}.json`;
    const staged = await stagePrivatePayload("tmp", basename, payload, t("DNS 规则私有载荷"));
    if (!staged) {
      if (!disposed) error.value = t("安全临时数据写入失败，DNS 规则未保存。");
      return;
    }
    try {
      if (disposed) return;
      const outcome = await runPrivateCli(`dns rules save-file ${shellQuote(staged.path)}`, t("校验并应用 DNS 规则"), "dns rules save-file [private-payload]");
      if (disposed) return;
      pendingApply.value = false;
      if (!outcome.ok) {
        error.value = t("DNS 规则校验或应用失败，未确认生效。请检查条件、重新读取规则或查看健康状态。");
        return;
      }
      draft.value = null;
      await loadSnapshot(true);
      if (!disposed) status.value = t("DNS 规则已校验、保存并应用。");
    } finally {
      const cleaned = await removePrivatePayload("tmp", staged.basename, t("DNS 规则私有载荷"));
      if (!cleaned && !disposed) error.value = t("DNS 规则临时载荷清理失败，请检查模块临时目录。");
    }
  });
}

onBeforeUnmount(() => { disposed = true; readSequence += 1; });
const { target: visibilityTarget } = useVisibilityTask(refresh);
</script>

<template>
  <section ref="visibilityTarget" class="dns-rules-panel min-w-0">
    <Card class="grid min-w-0 gap-3">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <h2 class="text-base font-semibold">{{ t("DNS 规则") }}</h2>
        <div class="flex flex-wrap gap-2">
          <Button size="sm" variant="outline" :disabled="busy || changed || Boolean(draft)" :loading="isRunning('dns-rules-load')" @click="refresh"><RefreshCw :size="14" />{{ t("重新读取规则") }}</Button>
          <Button size="sm" :disabled="!snapshot || snapshot.truncated || busy" @click="openDraft()"><Plus :size="14" />{{ t("添加 DNS 规则") }}</Button>
        </div>
      </div>
      <p class="text-sm leading-6 text-[var(--mn-ink-muted)]">{{ t("按顺序匹配 DNS 规则，普通解析规则命中后使用指定目标；未命中时使用下方默认解析器。DNS 解析目标不等于连接出站。") }}</p>
      <p class="text-xs leading-5 text-[var(--mn-ink-muted)]">{{ t("规则意图会独立保存，订阅和 Profile 切换后继续应用。节点解析器、模式规则和功能规则由模块维护；高级规则只读。") }}</p>
      <div v-if="snapshot" class="grid gap-1 text-xs text-[var(--mn-ink-muted)]">
        <p>{{ t("规则来源") }}：{{ snapshot.source === 'user_intent' ? t("已保存的用户规则意图") : t("当前配置") }}</p>
        <p>{{ t("应用 DNS Profile") }}：<code>{{ snapshot.applicationServer }}</code></p>
        <p>{{ t("未命中规则的默认解析器") }}：<code>{{ snapshot.finalServer || t("未知") }}</code></p>
        <p>{{ t("节点域名默认解析器（只读）") }}：<code>{{ snapshot.nodeResolver || t("未知") }}</code></p>
        <p>{{ t("这里只展示配置规则，不声称每条规则已被运行中的核心命中。") }}</p>
      </div>
      <p v-if="snapshot?.truncated" role="alert" class="text-sm text-[var(--mn-warning)]">{{ t("规则快照超过编辑上限，不能从截断数据保存。") }}</p>
      <p v-if="error" role="alert" class="text-sm leading-6 text-[var(--mn-danger)]">{{ error }}</p>
      <p v-if="status" role="status" class="text-sm leading-6 text-[var(--mn-ink-muted)]">{{ status }}</p>
      <ol class="grid min-w-0 gap-2">
        <li v-for="(row, index) in rows" :key="row.key" class="dns-rule-row grid min-w-0 gap-2 rounded-md border border-[color-mix(in_srgb,var(--mn-ink)_12%,transparent)] p-3">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <strong class="text-sm">{{ t("第 {order} 条规则", { order: index + 1 }) }} · {{ targetLabel(row.target) }}</strong>
            <div v-if="row.editable" class="flex flex-wrap gap-1">
              <Button size="sm" variant="outline" :disabled="busy || snapshot?.truncated" @click="openDraft(row)"><Pencil :size="13" />{{ t("调整") }}</Button>
              <Button size="sm" variant="outline" :aria-label="t('上移第 {order} 条 DNS 规则', { order: index + 1 })" :disabled="busy || !rows[index - 1]?.editable || snapshot?.truncated" @click="move(index, -1)"><ArrowUp :size="13" /></Button>
              <Button size="sm" variant="outline" :aria-label="t('下移第 {order} 条 DNS 规则', { order: index + 1 })" :disabled="busy || !rows[index + 1]?.editable || snapshot?.truncated" @click="move(index, 1)"><ArrowDown :size="13" /></Button>
              <Button size="sm" variant="outline" :aria-label="t('删除第 {order} 条 DNS 规则', { order: index + 1 })" :disabled="busy || !canRemove(index) || snapshot?.truncated" @click="remove(index)"><Trash2 :size="13" /></Button>
            </div>
            <span v-else class="text-xs text-[var(--mn-ink-muted)]">{{ protectedLabel(row) }}</span>
          </div>
          <pre class="min-w-0 whitespace-pre-wrap break-all text-xs leading-5 text-[var(--mn-ink-muted)]">{{ JSON.stringify(row.conditions, null, 2) }}</pre>
        </li>
      </ol>
      <form v-if="draft" class="dns-rule-editor grid gap-3 rounded-md bg-[var(--mn-surface-sunken)] p-3" @submit.prevent="saveDraft">
        <label class="grid min-w-0 gap-1 text-sm">{{ t("匹配条件（JSON）") }}<Textarea v-model="draft.conditions" :disabled="busy" spellcheck="false" rows="6" /></label>
        <label class="grid gap-1 text-sm">{{ t("DNS 解析目标") }}
          <select v-model="draft.target" class="mn-field h-10 min-w-0 rounded-md px-3" :disabled="busy">
            <option v-for="resolver in snapshot?.resolvers || []" :key="resolver.tag" :value="resolver.tag" :disabled="!resolver.available">{{ targetLabel(resolver.tag) }}</option>
          </select>
        </label>
        <div class="flex flex-wrap gap-2"><Button type="submit" size="sm" :disabled="busy">{{ t("保留规则修改") }}</Button><Button type="button" size="sm" variant="outline" :disabled="busy" @click="draft = null">{{ t("取消") }}</Button></div>
      </form>
      <div v-if="changed" class="flex flex-wrap gap-2">
        <Button :disabled="busy || Boolean(draft) || snapshot?.truncated" @click="requestApply">{{ t("校验并应用 DNS 规则") }}</Button>
        <Button variant="outline" :disabled="busy" @click="discard">{{ t("放弃未保存修改") }}</Button>
      </div>
      <ConfirmPanel v-if="pendingApply" :title="t('应用 DNS 规则修改')" :detail="t('设备会检查规则引用、受保护规则和 sing-box 完整配置；应用失败时尝试恢复原配置。运行中的核心可能重启。')" :loading="isRunning('dns-rules-save')" @confirm="apply" @cancel="!busy && (pendingApply = false)" />
    </Card>
  </section>
</template>
