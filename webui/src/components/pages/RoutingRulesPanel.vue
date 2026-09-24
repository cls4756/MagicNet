<script setup lang="ts">
import { computed, ref } from "vue";
import { Plus, RefreshCw, Pencil, Trash2 } from "lucide-vue-next";
import { t } from "@/i18n";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import Card from "@/components/ui/Card.vue";
import CardHeading from "@/components/ui/CardHeading.vue";
import ConfirmPanel from "@/components/ui/ConfirmPanel.vue";
import { parseRoutingInspectSnapshot, type RoutingInspectSnapshot, type RoutingRuleSummary } from "@/composables/routingInspect";
import { useActionLock } from "@/composables/useActionLock";
import { useMagicNet } from "@/composables/useMagicNet";
import { useVisibilityTask } from "@/composables/useVisibilityTask";
import { execFailed, shellQuote } from "@/utils";

type RuleRow = {
  key: string;
  rule: RoutingRuleSummary;
  domain: string | null;
};

type RuleDraft = {
  originalDomain: string | null;
  originalTarget: string | null;
  domain: string;
  target: string;
};

const routeTargets = ["proxy", "direct", "block", "warp"] as const;
const { runCli } = useMagicNet();
const { isRunning, withAction } = useActionLock();
const snapshot = ref<RoutingInspectSnapshot | null>(null);
const error = ref("");
const draft = ref<RuleDraft | null>(null);
const pendingDelete = ref<RuleRow | null>(null);

const rows = computed<RuleRow[]>(() => (snapshot.value?.rules || []).flatMap((rule) => {
  if (rule.source === "magicnet_custom_domain" && rule.managedDomains.length) {
    return rule.managedDomains.map((domain, index) => ({
      key: `managed-${rule.order}-${index}-${domain}`,
      rule,
      domain,
    }));
  }
  return [{ key: `rule-${rule.order}`, rule, domain: null } as RuleRow];
}));

async function loadSnapshot(): Promise<boolean> {
  const output = await runCli("--json routing inspect", t("读取当前路由规则"), true);
  const parsed = execFailed(output) ? null : parseRoutingInspectSnapshot(output);
  if (!parsed) {
    error.value = t("读取路由规则失败，请检查当前配置。");
    return false;
  }
  snapshot.value = parsed;
  error.value = "";
  return true;
}

async function refresh(): Promise<void> {
  await withAction("routing-inspect-refresh", loadSnapshot);
}

function formatConditions(rule: RoutingRuleSummary, domain: string | null): string {
  if (domain) return `domain_suffix = ${domain}`;
  const entries = Object.entries(rule.conditions);
  if (!entries.length) return t("无匹配条件");
  return entries.map(([key, value]) => `${key} = ${Array.isArray(value) ? value.join(", ") : String(value)}`).join(" · ");
}

function sourceLabel(rule: RoutingRuleSummary): string {
  return rule.source === "magicnet_custom_domain" ? t("MagicNet 自定义域名") : t("当前生效配置");
}

function outboundLabel(rule: RoutingRuleSummary): string {
  if (rule.managedTarget === "proxy") return "proxy-rule";
  return rule.outbound || t("未知");
}

function openNew(): void {
  draft.value = { originalDomain: null, originalTarget: null, domain: "", target: "proxy" };
}

function openEdit(row: RuleRow): void {
  if (!row.domain || !row.rule.managedTarget) return;
  draft.value = {
    originalDomain: row.domain,
    originalTarget: row.rule.managedTarget,
    domain: row.domain,
    target: row.rule.managedTarget,
  };
}

function validDraft(value: RuleDraft): boolean {
  return Boolean(value.domain.trim())
    && value.domain.length <= 253
    && !/\s/.test(value.domain)
    && routeTargets.includes(value.target as typeof routeTargets[number]);
}

async function saveDraft(): Promise<void> {
  const value = draft.value;
  if (!value || !validDraft(value)) {
    error.value = t("请输入有效域名后缀，并选择支持的出站。");
    return;
  }
  await withAction("routing-rule-save", async () => {
    const command = value.originalDomain && value.originalTarget
      ? `route edit-domain ${shellQuote(value.originalTarget)} ${shellQuote(value.target)} ${shellQuote(value.originalDomain)} ${shellQuote(value.domain.trim())}`
      : `route add-domain ${shellQuote(value.target)} ${shellQuote(value.domain.trim())}`;
    const output = await runCli(command, value.originalDomain ? t("修改指定网络路由") : t("添加指定网络路由"));
    if (execFailed(output)) {
      error.value = t("路由规则操作失败，请查看执行结果。");
      return;
    }
    draft.value = null;
    await loadSnapshot();
  });
}

async function deleteRule(): Promise<void> {
  const row = pendingDelete.value;
  if (!row?.domain || !row.rule.managedTarget) return;
  await withAction("routing-rule-delete", async () => {
    const output = await runCli(
      `route remove-domain ${shellQuote(row.rule.managedTarget!)} ${shellQuote(row.domain!)}`,
      t("删除指定网络路由"),
    );
    if (execFailed(output)) {
      error.value = t("路由规则操作失败，请查看执行结果。");
      return;
    }
    pendingDelete.value = null;
    await loadSnapshot();
  });
}

const { target: visibilityTarget } = useVisibilityTask(refresh);
</script>

<template>
  <div ref="visibilityTarget" class="mn-deferred-region">
    <Card class="grid gap-3">
      <CardHeading :title="t('路由规则')">
        <div class="flex gap-2">
          <Button size="sm" variant="outline" :loading="isRunning('routing-inspect-refresh')" @click="refresh">
            <RefreshCw :size="15" />{{ t("刷新") }}
          </Button>
          <Button size="sm" variant="secondary" @click="openNew">
            <Plus :size="15" />{{ t("添加域名规则") }}
          </Button>
        </div>
      </CardHeading>
      <p class="text-sm leading-5 text-[var(--mn-ink-muted)]">
        {{ t("表格按 sing-box 当前生效顺序展示。可编辑 MagicNet 自定义域名规则；其他规则由对应功能或配置来源管理。当前规则模型按目标聚合，不提供无效的拖动排序。") }}
      </p>
      <p v-if="snapshot?.runtimeState === 'unknown'" class="text-xs text-[var(--mn-warning)]">{{ t("当前运行选择未知；下表规则来自已生效配置。") }}</p>

      <div v-if="draft" class="routing-rule-editor">
        <label class="grid gap-1 text-sm">
          <span>{{ t("域名后缀") }}</span>
          <input v-model="draft.domain" class="routing-rule-input" autocomplete="off" :placeholder="t('例如 example.com')" />
        </label>
        <label class="grid gap-1 text-sm">
          <span>{{ t("目标出站") }}</span>
          <select v-model="draft.target" class="routing-rule-input">
            <option v-for="target in routeTargets" :key="target" :value="target">{{ target === "proxy" ? "proxy-rule" : target }}</option>
          </select>
        </label>
        <div class="flex gap-2">
          <Button size="sm" variant="outline" @click="draft = null">{{ t("取消") }}</Button>
          <Button size="sm" variant="secondary" :loading="isRunning('routing-rule-save')" @click="saveDraft">{{ t("保存并应用") }}</Button>
        </div>
      </div>

      <ConfirmPanel
        v-if="pendingDelete"
        :title="t('删除域名路由')"
        :detail="`${pendingDelete.domain} → ${outboundLabel(pendingDelete.rule)}`"
        command="route remove-domain <target> <domain-suffix>"
        :loading="isRunning('routing-rule-delete')"
        :confirm-label="t('确认删除')"
        confirm-variant="destructive"
        @cancel="pendingDelete = null"
        @confirm="deleteRule"
      />

      <p v-if="error" class="text-sm text-[var(--mn-danger)]">{{ error }}</p>

      <div class="overflow-x-auto">
        <table class="routing-rule-table w-full text-sm">
          <thead>
            <tr><th>{{ t("顺序") }}</th><th>{{ t("来源") }}</th><th>{{ t("匹配条件") }}</th><th>{{ t("目标出站") }}</th><th>{{ t("操作") }}</th></tr>
          </thead>
          <tbody>
            <tr v-for="row in rows" :key="row.key">
              <td class="font-mono">{{ row.rule.order }}</td>
              <td><Badge :tone="row.domain ? 'info' : 'neutral'">{{ sourceLabel(row.rule) }}</Badge></td>
              <td class="routing-rule-condition">{{ formatConditions(row.rule, row.domain) }}</td>
              <td class="font-mono">{{ outboundLabel(row.rule) }}</td>
              <td>
                <div v-if="row.domain" class="flex gap-1">
                  <Button size="sm" variant="ghost" :aria-label="t('编辑 {value}', { value: row.domain })" @click="openEdit(row)"><Pencil :size="14" /></Button>
                  <Button size="sm" variant="ghost" :aria-label="t('删除 {value}', { value: row.domain })" @click="pendingDelete = row"><Trash2 :size="14" /></Button>
                </div>
                <span v-else class="text-xs text-[var(--mn-ink-faint)]">{{ t("由来源管理") }}</span>
              </td>
            </tr>
            <tr v-if="snapshot?.finalOutbound">
              <td class="font-mono">{{ t("兜底") }}</td>
              <td><Badge>{{ t("sing-box 默认") }}</Badge></td>
              <td>{{ t("前序路由均未匹配") }}</td>
              <td class="font-mono">{{ snapshot.finalOutbound }}</td>
              <td><span class="text-xs text-[var(--mn-ink-faint)]">{{ t("由配置管理") }}</span></td>
            </tr>
            <tr v-if="!rows.length && !snapshot"><td colspan="5" class="mn-empty text-center">{{ error || t("正在读取路由配置…") }}</td></tr>
            <tr v-else-if="!rows.length && !snapshot?.finalOutbound"><td colspan="5" class="mn-empty text-center">{{ t("当前配置没有路由规则") }}</td></tr>
          </tbody>
        </table>
      </div>
      <p v-if="snapshot?.truncated" class="text-xs text-[var(--mn-warning)]">{{ t("配置规则数量超过展示上限，表格内容已截断。") }}</p>
    </Card>
  </div>
</template>

<style scoped>
.routing-rule-editor { display: grid; grid-template-columns: minmax(0, 1fr) minmax(140px, .6fr) auto; align-items: end; gap: 10px; border: 1px solid var(--mn-border); border-radius: 10px; padding: 10px; background: var(--mn-surface-sunken); }
.routing-rule-input { min-width: 0; border: 1px solid var(--mn-border); border-radius: 7px; padding: 7px 9px; color: var(--mn-ink); background: var(--mn-surface); }
.routing-rule-table th, .routing-rule-table td { padding: 8px 9px; border-bottom: 1px solid var(--mn-border); text-align: left; vertical-align: middle; }
.routing-rule-table th { color: var(--mn-ink-muted); font-size: 12px; font-weight: 600; white-space: nowrap; }
.routing-rule-condition { min-width: 240px; max-width: 520px; color: var(--mn-ink-soft); overflow-wrap: anywhere; }
@media (max-width: 640px) { .routing-rule-editor { grid-template-columns: minmax(0, 1fr); } }
</style>
