<script setup lang="ts">
import { t } from "@/i18n";
import { computed, ref, watch } from "vue";
import { ChevronDown, Copy, RefreshCw, Route } from "lucide-vue-next";
import Button from "@/components/ui/Button.vue";
import Card from "@/components/ui/Card.vue";
import ConfirmPanel from "@/components/ui/ConfirmPanel.vue";
import InsightChip from "@/components/ui/InsightChip.vue";
import SearchField from "@/components/ui/SearchField.vue";
import { buildNodeDelayStats, nodeDelayQualityLabel, parseNodeTestAll, sanitizeNodeText, type NodeDelayEntry } from "@/composables/nodeDelayParsers";
import { parseProxyGroupsSnapshot, sanitizeProxyName, type ProxyGroupSummary } from "@/composables/proxyGroupParsers";
import { useActionLock } from "@/composables/useActionLock";
import { useMagicNet } from "@/composables/useMagicNet";
import { useVisibilityTask } from "@/composables/useVisibilityTask";
import { copyText, execFailed, shellQuote } from "@/utils";
import { buildProxySelectionPlan, formatProxySelectionPlanReport, type ProxySelectionPlan } from "./proxySelectionPlan";

type PendingProxyAction = {
  group: ProxyGroupSummary;
  node: string;
  run: () => Promise<void>;
};

type ProxyView = "select" | "manual" | "auto";

const { runCli, state } = useMagicNet();
const { isRunning, withAction } = useActionLock();
const rawOutput = ref("");
const copied = ref(false);
const groupQuery = ref("");
const activeView = ref<ProxyView>("select");
const groupDelays = ref<Record<string, NodeDelayEntry[]>>({});
const expandedGroups = ref<Set<string>>(new Set());
const pendingAction = ref<PendingProxyAction | null>(null);
const selectionPlanCopied = ref(false);

const snapshot = computed(() => parseProxyGroupsSnapshot(rawOutput.value));
const pendingPlan = computed<ProxySelectionPlan | null>(() => {
  const action = pendingAction.value;
  if (!action) return null;
  return buildProxySelectionPlan(action.group, action.node, groupDelays.value[action.group.name] || []);
});
const allFilteredGroups = computed(() => {
  const query = groupQuery.value.trim().toLowerCase();
  const groups = snapshot.value?.groups || [];
  if (!query) return groups;
  return groups.filter((group) => [
    group.name,
    group.type,
    group.now,
    ...group.proxies
  ].some((value) => sanitizeProxyName(value).toLowerCase().includes(query)));
});
const visibleGroups = computed(() => allFilteredGroups.value.filter((group) => {
  if (activeView.value === "auto") return group.kind === "auto";
  if (activeView.value === "manual") return group.kind === "selector";
  return group.kind === "provider" || group.kind === "selector";
}));
const viewCounts = computed(() => ({
  select: allFilteredGroups.value.filter((group) => group.kind === "provider" || group.kind === "selector").length,
  manual: allFilteredGroups.value.filter((group) => group.kind === "selector").length,
  auto: allFilteredGroups.value.filter((group) => group.kind === "auto").length,
}));
const visibleNodes = computed(() => {
  const query = groupQuery.value.trim().toLowerCase();
  const nodes = snapshot.value?.nodes || [];
  if (!query) return nodes;
  return nodes.filter((node) => [node.name, node.type]
    .some((value) => sanitizeProxyName(value).toLowerCase().includes(query)));
});
const groupNames = computed(() => new Set((snapshot.value?.groups || []).map((group) => group.name)));

function groupKindLabel(group: ProxyGroupSummary): string {
  if (group.kind === "auto") return t("自动测速组");
  if (group.kind === "provider") return t("订阅提供商组");
  return t("手动策略组");
}

function memberKindLabel(node: string): string {
  if (["direct", "block", "dns", "tun"].includes(node.trim().toLowerCase())) return t("内置");
  return groupNames.value.has(node) ? t("组") : t("节点");
}

function viewLabel(view: ProxyView): string {
  if (view === "manual") return t("手动切换");
  if (view === "auto") return t("自动选择");
  return t("节点选择");
}

async function refreshGroups(): Promise<void> {
  await withAction("proxy-groups-refresh", async () => {
    copied.value = false;
    pendingAction.value = null;
    selectionPlanCopied.value = false;
    groupDelays.value = {};
    rawOutput.value = await runCli("api proxies", t("读取代理组"));
  });
}

function requestSelect(group: ProxyGroupSummary, node: string): void {
  if (!validProxyChoice(group.name, node)) {
    state.output = t("代理组或节点名称为空/过长，已拒绝执行。");
    return;
  }
  pendingAction.value = {
    group,
    node,
    run: () => selectNode(group.name, node)
  };
  selectionPlanCopied.value = false;
}

function validProxyChoice(group: string, node: string): boolean {
  return Boolean(group.trim() && node.trim() && group.length <= 180 && node.length <= 240);
}

async function testGroup(group: ProxyGroupSummary): Promise<void> {
  const nodes = group.proxies.slice(0, 16);
  if (!nodes.length) return;
  await withAction(`proxy-group-test-${group.name}`, async () => {
    const output = await runCli(`node test-all ${nodes.map(shellQuote).join(" ")}`, t("测速 {name}", { name: group.name }));
    const requested = new Set(nodes);
    const entries = parseNodeTestAll(output).filter((entry) => requested.has(entry.node));
    groupDelays.value = { ...groupDelays.value, [group.name]: entries };
  });
}

function requestUseFastest(group: ProxyGroupSummary): void {
  const fastest = groupDelayStats(group).fastest;
  if (!fastest) {
    state.output = t("请先测速本组，且至少需要一个可用节点。");
    return;
  }
  requestSelect(group, fastest.node);
}

function groupDelayStats(group: ProxyGroupSummary) {
  return buildNodeDelayStats(groupDelays.value[group.name] || []);
}

function visibleGroupNodes(group: ProxyGroupSummary): string[] {
  const query = groupQuery.value.trim().toLowerCase();
  if (!query) return expandedGroups.value.has(group.name) ? group.proxies : group.proxies.slice(0, 9);
  const groupMatched = [group.name, group.type, group.now]
    .some((value) => sanitizeProxyName(value).toLowerCase().includes(query));
  const nodes = groupMatched
    ? group.proxies
    : group.proxies.filter((node) => sanitizeProxyName(node).toLowerCase().includes(query));
  return nodes;
}

function toggleGroupExpanded(group: ProxyGroupSummary): void {
  const next = new Set(expandedGroups.value);
  if (next.has(group.name)) next.delete(group.name);
  else next.add(group.name);
  expandedGroups.value = next;
}

async function selectNode(group: string, node: string): Promise<void> {
  await withAction("proxy-groups-select", async () => {
    const text = await runCli(`api select ${shellQuote(group)} ${shellQuote(node)}`, t("切换代理节点"));
    if (execFailed(text)) return;
    state.output = text;
    const refreshed = await runCli("api proxies", t("刷新代理组"), true);
    if (execFailed(refreshed)) {
      state.output = t("代理节点切换已执行，但代理组刷新未确认：\n{refreshed}", { refreshed: refreshed });
      return;
    }
    rawOutput.value = refreshed;
  });
}

function cancelAction(): void {
  pendingAction.value = null;
  selectionPlanCopied.value = false;
}

async function confirmAction(): Promise<void> {
  const action = pendingAction.value;
  if (!action) return;
  try {
    await action.run();
  } finally {
    pendingAction.value = null;
    selectionPlanCopied.value = false;
  }
}

async function copySelectionPlan(): Promise<void> {
  const action = pendingAction.value;
  const plan = pendingPlan.value;
  if (!action || !plan) return;
  selectionPlanCopied.value = await copyText(formatProxySelectionPlanReport(plan));
  state.output = selectionPlanCopied.value ? t("代理切换计划摘要已复制。") : t("剪贴板不可用，代理切换计划未复制。");
}

async function copyReport(): Promise<void> {
  const report = visibleGroups.value.flatMap((group) => {
    const stats = groupDelayStats(group);
    const delays = groupDelays.value[group.name] || [];
    return [
      `${sanitizeProxyName(group.name)} type=${group.type} now=${sanitizeProxyName(group.now || "none")} count=${group.proxies.length}`,
      `delay_tested=${stats.tested} usable=${stats.usable} failed=${stats.failed} median_ms=${stats.medianMillis ?? "none"} parsed_usable_percent=${stats.usablePercent} fastest=${stats.fastest ? `${sanitizeNodeText(stats.fastest.node)} ${sanitizeNodeText(stats.fastest.summary)}` : "none"}`,
      ...visibleGroupNodes(group).map((node) => {
        const delay = delays.find((entry) => entry.node === node);
        return `  - ${sanitizeProxyName(node)}${delay ? ` delay=${sanitizeNodeText(delay.summary)} quality=${delay.quality}` : ""}`;
      })
    ];
  }).join("\n");
  copied.value = await copyText(report);
  state.output = copied.value ? t("代理组报告已复制。") : t("剪贴板不可用，代理组报告未复制。");
}

const { target: visibilityTarget } = useVisibilityTask(refreshGroups);

watch(() => state.subscriptions.lastSuccessEpoch, (value, previous) => {
  if (value > 0 && value !== previous && rawOutput.value) void refreshGroups();
});
</script>

<template>
  <div ref="visibilityTarget" class="mn-deferred-region">
    <Card class="grid gap-3">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div class="min-w-0">
          <h3 class="inline-flex items-center gap-2 text-base font-semibold"><Route :size="17" /> {{ t("代理") }}</h3>
          <p class="mt-1 text-sm leading-6 text-[var(--mn-ink-muted)]"> {{ t("按节点选择、手动切换和自动选择查看代理组，避免把运行时组误认为订阅来源。") }}
          </p>
        </div>
        <div class="flex gap-2">
          <Button size="sm" variant="outline" :loading="isRunning('proxy-groups-refresh')" @click="refreshGroups">
            <RefreshCw :size="15" />{{ t("刷新") }} </Button>
          <Button size="sm" variant="secondary" :disabled="!visibleGroups.length" @click="copyReport">
            <Copy :size="15" />{{ copied ? t("已复制") : t("复制") }}
          </Button>
        </div>
      </div>

    <nav class="proxy-view-tabs" :aria-label="t('代理分类')">
      <button v-for="view in (['select', 'manual', 'auto'] as ProxyView[])" :key="view" type="button" :class="activeView === view ? 'is-active' : ''" @click="activeView = view">
        <span>{{ viewLabel(view) }}</span>
        <small>{{ viewCounts[view] }}</small>
      </button>
    </nav>

    <div class="proxy-explanation">
      <p>{{ activeView === "auto" ? t("自动选择由 sing-box 根据测速结果决定出口，不需要逐个节点手动切换。") : activeView === "manual" ? t("手动切换用于明确指定一个策略组当前使用的节点。") : t("节点选择展示当前运行时可用的节点和策略组；组里的成员可能是节点，也可能是另一个组。") }}</p>
      <div class="proxy-metrics">
        <div><strong>{{ snapshot?.nodes.length || 0 }}</strong><span>{{ t("运行时节点") }}</span></div>
        <div><strong>{{ visibleGroups.length }}</strong><span>{{ viewLabel(activeView) }}</span></div>
        <div><strong>{{ state.subscriptions.lastImportedCount }}</strong><span>{{ t("最近导入") }}</span></div>
      </div>
    </div>

    <div class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
      <SearchField v-model="groupQuery" :placeholder="t('搜索代理组或节点')" />
      <span class="text-sm text-[var(--mn-ink-muted)]">
        {{ t("{visible} / {total} 组", { visible: visibleGroups.length, total: viewCounts[activeView] }) }} </span>
    </div>

    <section v-if="visibleNodes.length && activeView === 'select'" class="proxy-node-section" :aria-label="t('运行时节点')">
      <div class="proxy-subheading">
        <div>
          <h4>{{ t("运行时节点") }}</h4>
          <p>{{ t("这些是当前 API 返回的非策略组出站；节点可能同时属于多个组。") }}</p>
        </div>
        <span>{{ t("{value} 个", { value: visibleNodes.length }) }}</span>
      </div>
      <div class="proxy-node-grid">
        <div v-for="node in visibleNodes" :key="node.name" class="proxy-node-card">
          <span class="truncate">{{ sanitizeProxyName(node.name) }}</span>
          <span>{{ sanitizeProxyName(node.type) }}</span>
        </div>
      </div>
    </section>

    <ConfirmPanel
      v-if="pendingAction"
      :title="t('切换代理节点')"
      :detail="`${sanitizeProxyName(pendingAction.group.name)}：${sanitizeProxyName(pendingPlan?.summary || '')}`"
      command="api select <group> <node>"
      :loading="isRunning('proxy-groups-select')"
      :confirm-label="t('确认切换')"
      confirm-variant="secondary"
      @cancel="cancelAction"
      @confirm="confirmAction"
    >
      <div class="mt-3 flex flex-wrap gap-2">
        <InsightChip
          v-for="item in pendingPlan?.items || []"
          :key="item.label"
          :label="t(item.label)"
          :value="sanitizeProxyName(item.value)"
          :tone="item.tone"
        />
      </div>
      <p v-if="pendingPlan?.warnings.length" class="mt-2 text-xs leading-5 text-[var(--mn-warning)]/80">
        {{ pendingPlan.warnings.join("; ") }}
      </p>
      <template #actions>
        <Button size="sm" variant="outline" @click="cancelAction">{{ t("取消") }}</Button>
        <Button size="sm" variant="outline" @click="copySelectionPlan">{{ selectionPlanCopied ? t("已复制计划") : t("复制计划") }}</Button>
        <Button size="sm" variant="secondary" :loading="isRunning('proxy-groups-select')" @click="confirmAction">{{ t("确认切换") }}</Button>
      </template>
    </ConfirmPanel>

    <div v-if="visibleGroups.length" class="proxy-group-grid">
      <article v-for="group in visibleGroups" :key="group.name" class="proxy-group-card">
          <div class="proxy-group-card-header">
            <div class="min-w-0">
            <p class="proxy-group-title"><span class="proxy-group-symbol">{{ group.kind === "auto" ? "♻" : group.kind === "provider" ? "▣" : "☑" }}</span>{{ sanitizeProxyName(group.name) }}</p>
            <p class="proxy-group-type">{{ groupKindLabel(group) }} · {{ sanitizeProxyName(group.type) }}</p>
          </div>
          <span class="proxy-group-current">{{ group.selectable ? sanitizeProxyName(group.now || t("未选择")) : t("自动选择") }}</span>
        </div>
        <div class="proxy-group-card-meta">
          <p>
            <template v-if="groupDelayStats(group).tested"> {{ t("已测 {tested} · 可用 {usable} · 最快 {fastest}", { tested: groupDelayStats(group).tested, usable: groupDelayStats(group).usable, fastest: groupDelayStats(group).fastest?.summary || t("无") }) }}
            </template>
            <template v-else>{{ group.kind === "auto" ? t("由 sing-box 自动测速，不需要手动切换。") : t("可测速本组前 16 个节点。") }}</template>
          </p>
          <Button size="sm" variant="outline" :disabled="group.kind !== 'selector'" :loading="isRunning(`proxy-group-test-${group.name}`)" @click="testGroup(group)">{{ t("测速") }}</Button>
          <Button size="sm" variant="secondary" :disabled="!group.selectable || !groupDelayStats(group).fastest" @click="requestUseFastest(group)">{{ t("最快") }}</Button>
        </div>
        <div class="proxy-member-grid">
          <button
            v-for="node in visibleGroupNodes(group)"
            :key="`${group.name}-${node}`"
            class="proxy-member-card"
            :aria-pressed="node === group.now"
            :class="node === group.now ? 'mn-tone-ok' : 'border-[var(--mn-border)] text-[var(--mn-ink-soft)]'"
            type="button"
            :disabled="!group.selectable || node === group.now || isRunning('proxy-groups-select')"
            :title="group.selectable ? t('切换到 {value}', { value: sanitizeProxyName(node) }) : t('自动测速组由 sing-box 自动选择')"
            @click="requestSelect(group, node)"
          >
            <span class="truncate">{{ sanitizeProxyName(node) }}</span>
            <span class="mt-1 text-xs text-[var(--mn-ink-faint)]">{{ memberKindLabel(node) }}</span>
            <span v-if="groupDelays[group.name]?.find((entry) => entry.node === node)" class="mt-1 text-xs text-[var(--mn-ink-muted)]">
              {{ sanitizeNodeText(groupDelays[group.name].find((entry) => entry.node === node)?.summary || "") }}
              · {{ nodeDelayQualityLabel(groupDelays[group.name].find((entry) => entry.node === node)?.quality || "failed") }}
            </span>
          </button>
        </div>
        <Button
          v-if="group.proxies.length > 9 && !groupQuery.trim()"
          class="mt-2"
          size="sm"
          variant="ghost"
          :aria-expanded="expandedGroups.has(group.name)"
          @click="toggleGroupExpanded(group)"
        >
          <ChevronDown :size="15" :class="expandedGroups.has(group.name) ? 'rotate-180' : ''" />
          {{ expandedGroups.has(group.name) ? t("收起节点") : t("查看全部 {count} 个节点", { count: group.proxies.length }) }}
        </Button>
      </article>
    </div>
    <pre v-else-if="rawOutput" class="max-h-48 overflow-auto rounded-md bg-[var(--mn-carrier-deep)] p-3 text-xs leading-6 text-[var(--mn-ink-soft)] whitespace-pre-wrap">{{ rawOutput }}</pre>
    <p v-else class="mn-empty">{{ t("正在读取当前策略组与节点；如果 sing-box 未运行，请先更新订阅并启动服务。") }}</p>
    </Card>
  </div>
</template>

<style scoped>
.proxy-explanation { display: grid; gap: 12px; border: 1px solid var(--mn-border); border-radius: var(--mn-radius-sm); padding: 14px; background: var(--mn-surface-sunken); }
.proxy-explanation p { margin: 0; color: var(--mn-ink-muted); font-size: .8125rem; line-height: 1.65; }
.proxy-view-tabs { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; border-bottom: 1px solid var(--mn-border); }
.proxy-view-tabs button { display: flex; align-items: center; justify-content: center; gap: 7px; min-height: 52px; border: 0; border-bottom: 3px solid transparent; padding: 10px 6px; color: var(--mn-ink-muted); background: transparent; font-size: .875rem; font-weight: 550; }
.proxy-view-tabs button:hover { color: var(--mn-ink); }
.proxy-view-tabs button.is-active { border-bottom-color: var(--mn-clay); color: var(--mn-clay-ink); }
.proxy-view-tabs small { display: inline-grid; place-items: center; min-width: 21px; height: 21px; border-radius: 999px; color: var(--mn-ink-muted); background: var(--mn-carrier); font-size: .6875rem; font-weight: 650; }
.proxy-view-tabs button.is-active small { color: var(--mn-clay-ink); background: var(--mn-coral); }
.proxy-metrics { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; }
.proxy-metrics div { display: grid; gap: 3px; min-width: 0; }
.proxy-metrics strong { color: var(--mn-ink); font-size: 1.25rem; font-weight: 600; font-variant-numeric: tabular-nums; }
.proxy-metrics span { color: var(--mn-ink-muted); font-size: .75rem; }
.proxy-node-section { display: grid; gap: 10px; border-top: 1px solid var(--mn-border); padding-top: 14px; }
.proxy-subheading { display: flex; align-items: end; justify-content: space-between; gap: 12px; }
.proxy-subheading h4 { margin: 0; color: var(--mn-ink); font-size: .9375rem; font-weight: 600; }
.proxy-subheading p { margin: 4px 0 0; color: var(--mn-ink-muted); font-size: .8125rem; line-height: 1.55; }
.proxy-subheading > span { flex: 0 0 auto; color: var(--mn-ink-muted); font-size: .8125rem; }
.proxy-node-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; }
.proxy-node-card { display: grid; gap: 4px; min-width: 0; border: 1px solid var(--mn-border); border-radius: var(--mn-radius-sm); padding: 10px 11px; color: var(--mn-ink-soft); background: var(--mn-ivory); font-size: .8125rem; }
.proxy-node-card > span:last-child { color: var(--mn-ink-muted); font-size: .75rem; }
.proxy-group-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
.proxy-group-card { display: grid; gap: 14px; min-width: 0; border: 1px solid var(--mn-border); border-radius: 18px; padding: 16px; background: var(--mn-ivory); }
.proxy-group-card-header { display: flex; align-items: flex-start; justify-content: space-between; gap: 10px; min-width: 0; }
.proxy-group-title { display: flex; align-items: center; gap: 8px; min-width: 0; margin: 0; color: var(--mn-ink); font-size: 1rem; font-weight: 650; overflow-wrap: anywhere; }
.proxy-group-symbol { display: inline-grid; place-items: center; flex: 0 0 auto; width: 25px; height: 25px; border-radius: 7px; color: var(--mn-clay-ink); background: var(--mn-coral); font-size: 1rem; }
.proxy-group-type { margin: 5px 0 0 33px; color: var(--mn-ink-muted); font-size: .8125rem; }
.proxy-group-current { max-width: 46%; border-radius: 8px; padding: 5px 8px; color: var(--mn-ink-soft); background: var(--mn-carrier); font-size: .75rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.proxy-group-card-meta { display: grid; grid-template-columns: minmax(0, 1fr) auto auto; align-items: center; gap: 7px; }
.proxy-group-card-meta p { min-width: 0; margin: 0; color: var(--mn-ink-muted); font-size: .75rem; line-height: 1.5; }
.proxy-member-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
.proxy-member-card { display: grid; gap: 4px; min-width: 0; min-height: 60px; border: 1px solid var(--mn-border); border-radius: 10px; padding: 10px; color: var(--mn-ink-soft); background: var(--mn-surface-sunken); text-align: left; font-size: .8125rem; }
.proxy-member-card:hover:not(:disabled) { border-color: var(--mn-primary); }
.proxy-member-card[aria-pressed="true"] { border-color: var(--mn-success); background: var(--mn-tone-ok-bg); }
.proxy-member-card > span:nth-child(2) { color: var(--mn-ink-faint); font-size: .6875rem; }
@media (max-width: 640px) {
  .proxy-metrics, .proxy-node-grid, .proxy-group-grid { grid-template-columns: minmax(0, 1fr); }
  .proxy-group-card-meta { grid-template-columns: minmax(0, 1fr) auto; }
  .proxy-group-card-meta p { grid-column: 1 / -1; }
  .proxy-member-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .proxy-subheading { align-items: start; }
}
</style>
