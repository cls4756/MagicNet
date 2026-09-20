<script setup lang="ts">
import { t } from "@/i18n";
import { RefreshCw } from "lucide-vue-next";

import { computed, onMounted, ref } from "vue";
import Button from "@/components/ui/Button.vue";
import Card from "@/components/ui/Card.vue";
import ConfirmPanel from "@/components/ui/ConfirmPanel.vue";
import InsightChip from "@/components/ui/InsightChip.vue";
import PageHeader from "@/components/ui/PageHeader.vue";
import SearchField from "@/components/ui/SearchField.vue";
import { useActionLock } from "@/composables/useActionLock";
import { devicePackageIconsAvailable } from "@/composables/devicePackages";
import { useMagicNet } from "@/composables/useMagicNet";
import { bytesToBase64, execFailed, redactedCliPreview } from "@/utils";
import AppPolicyRouteGuide from "./AppPolicyRouteGuide.vue";
import { comparePackagesByLabel, packageDisplayName, packageIconUrl, packageInitial } from "./appPackageList";
import {
  appPolicyListDeltas,
  appPolicyPendingCount,
  appPolicySyncPayload,
  isAppPolicyTargetSelected,
  normalizeAppPolicyLists,
  sameAppPolicyLists,
  toggleAppPolicyTarget,
  type AppPolicyLists,
  type AppPolicyTarget,
} from "./appPolicyDraft";
import type { PackageInfo } from "@/types";
import { buildAppPolicySummary } from "./appPolicyInsights";
import { buildAppPolicyChangePlan, type AppPolicyChangeOperation, type AppPolicyChangePlan } from "./appPolicyChangePlan";

const { state, runCli, refreshApps, refreshPackages, shellQuote } = useMagicNet();
const { isRunning, withAction } = useActionLock();
const pendingAppAction = ref<PendingAppAction | null>(null);
const pendingLists = ref<AppPolicyLists | null>(null);
const proxySearchQuery = ref("");
const directSearchQuery = ref("");

type PendingAppAction = {
  key: string;
  command: string;
  message: string;
  plan: AppPolicyChangePlan;
  run: () => Promise<void>;
};

const installedNames = computed(() => new Set(state.packages.map((item) => item.packageName)));

const packageIconsAvailable = devicePackageIconsAvailable();
const appIcon = (packageName: string): string | null =>
  packageIconUrl(packageName, packageIconsAvailable);

type ListApp = {
  info: PackageInfo;
  initial: string;
};

/** Device state as last read. Checkbox edits only overlay it until they apply. */
const appliedLists = computed<AppPolicyLists>(() =>
  normalizeAppPolicyLists(state.appPolicy.proxy, state.appPolicy.direct));

const draftLists = computed<AppPolicyLists>(() => pendingLists.value ?? appliedLists.value);
const pendingCount = computed(() =>
  pendingLists.value ? appPolicyPendingCount(appliedLists.value, pendingLists.value) : 0);

/** Applied entries first, then a stable alphabetical order; the draft never reorders rows. */
function buildListApps(target: AppPolicyTarget, searchQuery: string): ListApp[] {
  const applied = new Set(state.appPolicy[target]);
  const needle = searchQuery.trim().toLowerCase();
  return state.packages
    .filter((app) => !needle
      || app.packageName.toLowerCase().includes(needle)
      || packageDisplayName(app).toLowerCase().includes(needle))
    .sort((a, b) => {
      const aApplied = applied.has(a.packageName);
      const bApplied = applied.has(b.packageName);
      if (aApplied !== bApplied) return aApplied ? -1 : 1;
      return comparePackagesByLabel(a, b);
    })
    .map((app): ListApp => ({ info: app, initial: packageInitial(app) }));
}

const proxyListApps = computed(() => buildListApps("proxy", proxySearchQuery.value));
const directListApps = computed(() => buildListApps("direct", directSearchQuery.value));

const policySummary = computed(() => buildAppPolicySummary(
  state.appPolicy.mode,
  state.appPolicy.proxy,
  state.appPolicy.direct,
  installedNames.value,
));

function actionPlan(operation: AppPolicyChangeOperation): AppPolicyChangePlan {
  return buildAppPolicyChangePlan({
    mode: state.appPolicy.mode,
    proxy: state.appPolicy.proxy,
    direct: state.appPolicy.direct,
    bypass: [],
    installedPackages: installedNames.value
  }, operation);
}

function commandFailed(text: string): boolean {
  return execFailed(text);
}

function isSelected(pkg: string, target: AppPolicyTarget): boolean {
  return isAppPolicyTargetSelected(draftLists.value, target, pkg);
}

function isPending(pkg: string, target: AppPolicyTarget): boolean {
  if (!pendingLists.value) return false;
  return isSelected(pkg, target) !== state.appPolicy[target].includes(pkg);
}

function toggleApp(pkg: string, target: AppPolicyTarget): void {
  const next = toggleAppPolicyTarget(draftLists.value, target, pkg);
  pendingLists.value = sameAppPolicyLists(next, appliedLists.value) ? null : next;
}

async function applyPendingLists(): Promise<void> {
  const draft = pendingLists.value;
  if (!draft) return;
  const deltas = appPolicyListDeltas(appliedLists.value, draft);
  if (!deltas.length) return;
  const changed = deltas.reduce((total, delta) => total + delta.added.length + delta.removed.length, 0);
  await withAction("apply-app-lists", async () => {
    const previousProxy = [...state.appPolicy.proxy];
    const previousDirect = [...state.appPolicy.direct];
    state.output = t('正在应用名单更改（{count} 项）...', { count: changed });
    const payload = bytesToBase64(new TextEncoder().encode(appPolicySyncPayload(deltas)));
    const text = await runCli(
      `app sync ${shellQuote(payload)}`,
      t('应用名单更改'),
      true,
      redactedCliPreview("app sync [payload]"),
    );
    if (commandFailed(text)) {
      // Keep the draft so the user can fix and retry without re-checking rows.
      state.output = text;
      return;
    }
    pendingLists.value = null;
    if (!(await refreshApps(true))) {
      state.appPolicy.proxy = previousProxy;
      state.appPolicy.direct = previousDirect;
      return;
    }
    state.output = t('已应用名单更改，当前核心已重启。');
  });
}

async function setMode(mode: "blacklist" | "whitelist"): Promise<void> {
  await withAction(`mode-${mode}`, async () => {
    const text = await runCli(`app mode ${mode}`, mode === "blacklist" ? t('切换全局接管') : t('切换仅名单接管'));
    if (commandFailed(text)) return;
    await refreshApps(true);
  });
}

async function reapplyAppPolicy(): Promise<void> {
  await withAction("reapply-app-policy", async () => {
    const text = await runCli("app apply", t('重新解析 App UID 并套用策略'));
    if (commandFailed(text)) return;
    await refreshApps(true);
  });
}

function requestReapplyAppPolicy(): void {
  pendingAppAction.value = {
    key: "reapply-app-policy",
    command: "app apply",
    message: t('确认按当前已安装应用重新解析 UID 并套用现有策略？名单不会改变，但当前核心会重启。'),
    plan: actionPlan({ type: "reapply" }),
    run: reapplyAppPolicy,
  };
}

function requestSetMode(mode: "blacklist" | "whitelist"): void {
  pendingAppAction.value = {
    key: `mode-${mode}`,
    command: `app mode ${mode}`,
    message: mode === "blacklist"
      ? t('确认切换到全局接管？未列出应用会进入当前透明数据面。')
      : t('确认切换到仅名单接管？只有 Proxy 和 Direct 名单进入当前透明数据面，未列出应用走系统网络。'),
    plan: actionPlan({ type: "mode", mode }),
    run: () => setMode(mode)
  };
}

/** One button: re-read the device lists, the installed package catalog, and drop the draft. */
async function refreshAppList(): Promise<void> {
  await withAction("refresh-app-list", async () => {
    // The device state becomes the source of truth again, so any unapplied edits are discarded.
    pendingLists.value = null;
    await refreshApps();
    await refreshPackages();
  });
}

function cancelAppAction(): void {
  pendingAppAction.value = null;
}

async function confirmAppAction(): Promise<void> {
  const action = pendingAppAction.value;
  if (!action) return;
  // Consume only this confirmation; a new one may be opened while it runs.
  pendingAppAction.value = null;
  await action.run();
}

onMounted(() => {
  if (!state.packages.length) void refreshPackages(true);
});
</script>
<template>
  <div class="grid gap-4">
    <PageHeader :overline="t('应用策略')" :title="t('应用名单')" />
    <Teleport to="body">
      <Transition name="sheet">
        <div v-if="pendingAppAction" class="fixed inset-0 z-[70]" role="presentation">
          <button
            class="mn-overlay absolute inset-0 size-full"
            type="button"
            :aria-label="t('取消应用策略操作')"
            @click="cancelAppAction"
          />
          <div
            class="absolute inset-x-3 bottom-[max(1rem,env(safe-area-inset-bottom))] mx-auto max-h-[min(80dvh,680px)] max-w-3xl overflow-auto"
            role="dialog"
            aria-modal="true"
            aria-labelledby="app-policy-confirm-title"
          >
            <ConfirmPanel
              :title="t('确认应用策略')"
              :detail="pendingAppAction.message"
              :command="pendingAppAction.command"
              :loading="isRunning(pendingAppAction.key)"
              :confirm-label="t('应用更改')"
              confirm-variant="default"
              @cancel="cancelAppAction"
              @confirm="confirmAppAction"
            >
              <p id="app-policy-confirm-title" class="sr-only">{{ pendingAppAction.message }}</p>
              <div class="mt-3 flex flex-wrap gap-2">
                <InsightChip
                  v-for="item in pendingAppAction.plan.items"
                  :key="item.label"
                  :label="item.label"
                  :value="item.value"
                  :tone="item.tone"
                />
              </div>
              <p v-if="pendingAppAction.plan.warnings.length" class="mt-2 text-xs leading-5 text-[var(--mn-warning)]/80">
                {{ pendingAppAction.plan.warnings.join("；") }}
              </p>
              <div class="mt-3 grid gap-2 rounded-[var(--mn-radius-md)] border border-[var(--mn-border)] bg-[var(--mn-surface-raised)] p-3 text-xs leading-5">
                <p class="font-semibold text-[var(--mn-ink)]">{{ pendingAppAction.plan.routePreview.subject }}</p>
                <dl class="grid gap-1 text-[var(--mn-ink-muted)] sm:grid-cols-[auto_minmax(0,1fr)] sm:gap-x-3">
                  <dt class="font-semibold text-[var(--mn-ink-soft)]">{{ t('修改前') }}</dt>
                  <dd>{{ pendingAppAction.plan.routePreview.before }}</dd>
                  <dt class="font-semibold text-[var(--mn-ink-soft)]">{{ t('修改后') }}</dt>
                  <dd>{{ pendingAppAction.plan.routePreview.after }}</dd>
                  <dt class="font-semibold text-[var(--mn-ink-soft)]">{{ t('数据') }}</dt>
                  <dd>{{ pendingAppAction.plan.routePreview.traffic }}</dd>
                  <dt class="font-semibold text-[var(--mn-ink-soft)]">DNS</dt>
                  <dd>{{ pendingAppAction.plan.routePreview.dns }}</dd>
                  <dt class="font-semibold text-[var(--mn-ink-soft)]">{{ t('生效') }}</dt>
                  <dd>{{ pendingAppAction.plan.routePreview.activation }}</dd>
                </dl>
              </div>
            </ConfirmPanel>
          </div>
        </div>
      </Transition>
    </Teleport>

    <Card class="grid gap-3">
      <div class="flex flex-wrap items-center gap-3">
        <div class="mn-segmented">
          <button class="min-h-12 whitespace-nowrap rounded px-3 text-sm font-medium text-[var(--mn-ink-muted)] transition-colors disabled:cursor-progress disabled:opacity-60" :disabled="isRunning('mode-blacklist') || state.appPolicy.mode === 'blacklist'" :class="{ 'bg-[var(--mn-cactus)] text-[var(--mn-on-accent)]': state.appPolicy.mode === 'blacklist' }" @click="requestSetMode('blacklist')">{{ t('全局接管') }}</button>
          <button class="min-h-12 whitespace-nowrap rounded px-3 text-sm font-medium text-[var(--mn-ink-muted)] transition-colors disabled:cursor-progress disabled:opacity-60" :disabled="isRunning('mode-whitelist') || state.appPolicy.mode === 'whitelist'" :class="{ 'bg-[var(--mn-cactus)] text-[var(--mn-on-accent)]': state.appPolicy.mode === 'whitelist' }" @click="requestSetMode('whitelist')">{{ t('仅名单接管') }}</button>
        </div>
        <span class="text-sm text-[var(--mn-ink-muted)]">
          {{ state.appPolicy.mode === 'whitelist'
            ? t('仅接管名单中的应用。')
            : t('接管全部应用，绕过名单除外。') }}
        </span>
      </div>
      <p class="text-xs leading-5 text-[var(--mn-ink-faint)]">{{ t('同一 Android UID 的应用会一起生效。') }}</p>
      <AppPolicyRouteGuide
        :mode="state.appPolicy.mode"
        :reapply-loading="isRunning('reapply-app-policy')"
        @reapply="requestReapplyAppPolicy"
      />
      <p v-if="policySummary.conflicts.length" role="alert" class="text-sm text-[var(--mn-danger)]">
        {{ t('{count} 个名单冲突，请检查下方分流概览。', { count: policySummary.conflicts.length }) }}
      </p>
    </Card>

    <!-- Proxy list box -->
    <Card class="grid gap-3">
      <div class="flex flex-wrap items-start justify-between gap-2">
        <h3 class="self-center text-base font-medium">{{ t('代理名单') }}</h3>
        <div class="flex flex-wrap items-center gap-2">
          <Button variant="outline" :loading="isRunning('refresh-app-list')" @click="refreshAppList"><RefreshCw :size="17" />{{ t('刷新') }}</Button>
          <Button
            :disabled="!pendingCount"
            :loading="isRunning('apply-app-lists')"
            @click="applyPendingLists"
          >{{ pendingCount ? `${t('应用更改')} (${pendingCount})` : t('应用更改') }}</Button>
        </div>
      </div>
      <p v-if="pendingCount" class="text-xs leading-5 text-[var(--mn-warning)]">
        {{ t('勾选后点“应用更改”才会写入并重启当前核心；在此之前不影响设备。') }}
      </p>
      <SearchField v-model="proxySearchQuery" :placeholder="t('搜索应用名称或包名')" />
      <div class="max-h-80 overflow-y-auto overscroll-contain rounded-[var(--mn-radius-md)] border border-[var(--mn-border)] bg-[var(--mn-surface-raised)]">
        <ul class="divide-y divide-[var(--mn-border)]">
          <li
            v-for="app in proxyListApps"
            :key="app.info.packageName"
            class="px-3 py-2"
            :class="{ 'bg-[color-mix(in_srgb,var(--mn-warning)_10%,transparent)]': isPending(app.info.packageName, 'proxy') }"
          >
            <label class="flex min-w-0 cursor-pointer items-center gap-2">
              <input
                type="checkbox"
                class="size-4 shrink-0 accent-[var(--mn-cactus)]"
                :checked="isSelected(app.info.packageName, 'proxy')"
                :aria-label="t('选择 {value}', { value: packageDisplayName(app.info) })"
                @change="toggleApp(app.info.packageName, 'proxy')"
              >
              <img
                v-if="appIcon(app.info.packageName)"
                :src="appIcon(app.info.packageName) ?? ''"
                alt=""
                class="size-8 shrink-0 rounded-md"
                loading="lazy"
                decoding="async"
              >
              <span v-else class="flex size-8 shrink-0 items-center justify-center rounded-md bg-[var(--mn-ivory)] text-xs font-medium text-[var(--mn-ink-muted)]" aria-hidden="true">{{ app.initial }}</span>
              <span class="grid min-w-0 gap-0.5">
                <span class="min-w-0 break-all text-sm text-[var(--mn-ink-soft)]">{{ packageDisplayName(app.info) }}</span>
                <span v-if="packageDisplayName(app.info) !== app.info.packageName" class="min-w-0 break-all text-xs text-[var(--mn-ink-faint)]">{{ app.info.packageName }}</span>
              </span>
            </label>
          </li>
        </ul>
        <p v-if="!proxyListApps.length" class="mn-empty p-3">{{ state.packages.length ? t('没有匹配的应用。') : t('暂无应用，点“刷新”读取。') }}</p>
      </div>
    </Card>

    <!-- Direct list box -->
    <Card class="grid gap-3">
      <div class="flex flex-wrap items-start justify-between gap-2">
        <h3 class="self-center text-base font-medium">{{ t('MagicNet 内直连') }}</h3>
        <div class="flex flex-wrap items-center gap-2">
          <Button variant="outline" :loading="isRunning('refresh-app-list')" @click="refreshAppList"><RefreshCw :size="17" />{{ t('刷新') }}</Button>
          <Button
            :disabled="!pendingCount"
            :loading="isRunning('apply-app-lists')"
            @click="applyPendingLists"
          >{{ pendingCount ? `${t('应用更改')} (${pendingCount})` : t('应用更改') }}</Button>
        </div>
      </div>
      <SearchField v-model="directSearchQuery" :placeholder="t('搜索应用名称或包名')" />
      <div class="max-h-80 overflow-y-auto overscroll-contain rounded-[var(--mn-radius-md)] border border-[var(--mn-border)] bg-[var(--mn-surface-raised)]">
        <ul class="divide-y divide-[var(--mn-border)]">
          <li
            v-for="app in directListApps"
            :key="app.info.packageName"
            class="px-3 py-2"
            :class="{ 'bg-[color-mix(in_srgb,var(--mn-warning)_10%,transparent)]': isPending(app.info.packageName, 'direct') }"
          >
            <label class="flex min-w-0 cursor-pointer items-center gap-2">
              <input
                type="checkbox"
                class="size-4 shrink-0 accent-[var(--mn-cactus)]"
                :checked="isSelected(app.info.packageName, 'direct')"
                :aria-label="t('选择 {value}', { value: packageDisplayName(app.info) })"
                @change="toggleApp(app.info.packageName, 'direct')"
              >
              <img
                v-if="appIcon(app.info.packageName)"
                :src="appIcon(app.info.packageName) ?? ''"
                alt=""
                class="size-8 shrink-0 rounded-md"
                loading="lazy"
                decoding="async"
              >
              <span v-else class="flex size-8 shrink-0 items-center justify-center rounded-md bg-[var(--mn-ivory)] text-xs font-medium text-[var(--mn-ink-muted)]" aria-hidden="true">{{ app.initial }}</span>
              <span class="grid min-w-0 gap-0.5">
                <span class="min-w-0 break-all text-sm text-[var(--mn-ink-soft)]">{{ packageDisplayName(app.info) }}</span>
                <span v-if="packageDisplayName(app.info) !== app.info.packageName" class="min-w-0 break-all text-xs text-[var(--mn-ink-faint)]">{{ app.info.packageName }}</span>
              </span>
            </label>
          </li>
        </ul>
        <p v-if="!directListApps.length" class="mn-empty p-3">{{ state.packages.length ? t('没有匹配的应用。') : t('暂无应用，点“刷新”读取。') }}</p>
      </div>
    </Card>

    <details class="mn-disclosure" :open="policySummary.conflicts.length > 0">
      <summary>{{ t('分流概览') }}<span v-if="policySummary.conflicts.length">{{ t('需要检查') }}</span></summary>
      <Card class="grid gap-3">
        <p class="text-sm leading-6 text-[var(--mn-ink-muted)]">{{ policySummary.summary }}</p>
        <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
          <InsightChip v-for="item in policySummary.items" :key="item.label" :label="item.label" :value="item.value" :tone="item.tone" />
        </div>
        <p v-if="policySummary.conflicts.length" class="break-all text-xs text-[var(--mn-danger)]">
          {{ t('冲突包名：') }}{{ policySummary.conflicts.join(", ") }}
        </p>
      </Card>
    </details>
  </div>
</template>
