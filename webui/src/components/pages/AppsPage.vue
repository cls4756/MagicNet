<script setup lang="ts">
import { t } from "@/i18n";
import { Copy, ListFilter, RefreshCw } from "lucide-vue-next";

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
import { copyText, execFailed, redactedCliPreview } from "@/utils";
import AppPolicyRouteGuide from "./AppPolicyRouteGuide.vue";
import { packageDisplayName, packageIconUrl, packageInitial } from "./appPackageList";
import type { PackageInfo } from "@/types";
import { buildAppPolicySummary, formatAppPolicyFullReport, formatAppPolicySafeReport } from "./appPolicyInsights";
import { buildAppPolicyChangePlan, type AppPolicyChangeOperation, type AppPolicyChangePlan } from "./appPolicyChangePlan";

const { state, runCli, refreshApps, refreshPackages, shellQuote } = useMagicNet();
const { isRunning, withAction } = useActionLock();
const pendingAppAction = ref<PendingAppAction | null>(null);
const appReportCopied = ref(false);
const safeReportCopied = ref(false);
const proxySearchQuery = ref("");
const directSearchQuery = ref("");

type PendingAppAction = {
  key: string;
  command: string;
  message: string;
  plan: AppPolicyChangePlan;
  run: () => Promise<void>;
};
type AppTarget = "proxy" | "direct";

const installedNames = computed(() => new Set(state.packages.map((item) => item.packageName)));

const packageIconsAvailable = devicePackageIconsAvailable();
const appIcon = (packageName: string): string | null =>
  packageIconUrl(packageName, packageIconsAvailable);

type ListApp = {
  info: PackageInfo;
  inList: boolean;
  initial: string;
};

function buildListApps(target: AppTarget, searchQuery: string): ListApp[] {
  const list = new Set(state.appPolicy[target]);
  const needle = searchQuery.trim().toLowerCase();
  const filtered = state.packages.filter((app) => {
    if (!needle) return true;
    return app.packageName.toLowerCase().includes(needle)
      || packageDisplayName(app).toLowerCase().includes(needle);
  });
  return filtered.sort((a, b) => {
    const aInList = list.has(a.packageName);
    const bInList = list.has(b.packageName);
    if (aInList && !bInList) return -1;
    if (!aInList && bInList) return 1;
    return 0;
  }).map((app): ListApp => ({
    info: app,
    inList: list.has(app.packageName),
    initial: packageInitial(app),
  }));
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

function targetList(target: AppTarget): string[] {
  return state.appPolicy[target];
}

function moveLocalPackage(pkg: string, target: AppTarget): void {
  state.appPolicy.proxy = state.appPolicy.proxy.filter((item) => item !== pkg);
  state.appPolicy.direct = state.appPolicy.direct.filter((item) => item !== pkg);
  targetList(target).push(pkg);
}

function isInAppList(pkg: string, target: AppTarget): boolean {
  return state.appPolicy[target].includes(pkg);
}

async function toggleAppList(pkg: string, target: AppTarget): Promise<void> {
  if (isInAppList(pkg, target)) {
    await removeApp(pkg, target);
  } else {
    await addPackage(pkg, target);
  }
}

async function addPackage(pkg: string, target: AppTarget, key = `add-${target}`): Promise<void> {
  await withAction(key, async () => {
    const previousProxy = [...state.appPolicy.proxy];
    const previousDirect = [...state.appPolicy.direct];
    moveLocalPackage(pkg, target);
    state.output = t('已加入界面，正在保存 {pkg}...', { pkg: pkg });
    const text = await runCli(
      `app add ${shellQuote(pkg)} ${target}`,
      t('添加应用 {pkg}', { pkg: pkg }),
      true,
      redactedCliPreview(`app add [package] ${target}`),
    );
    if (commandFailed(text)) {
      state.output = text;
      state.appPolicy.proxy = previousProxy;
      state.appPolicy.direct = previousDirect;
      return;
    }
    if (!(await refreshApps(true))) {
      state.appPolicy.proxy = previousProxy;
      state.appPolicy.direct = previousDirect;
    }
  });
}

async function removeApp(pkg: string, target: AppTarget): Promise<void> {
  await withAction(`remove-${target}-${pkg}`, async () => {
    const previousProxy = [...state.appPolicy.proxy];
    const previousDirect = [...state.appPolicy.direct];
    if (target === "proxy") {
      state.appPolicy.proxy = state.appPolicy.proxy.filter((item) => item !== pkg);
    } else if (target === "direct") {
      state.appPolicy.direct = state.appPolicy.direct.filter((item) => item !== pkg);
    }
    state.output = t('已从界面移除 {pkg}，正在后台保存...', { pkg: pkg });
    const text = await runCli(
      `app remove ${shellQuote(pkg)} ${target}`,
      t('移除应用 {pkg}', { pkg: pkg }),
      true,
      redactedCliPreview(`app remove [package] ${target}`),
    );
    if (commandFailed(text)) {
      state.output = text;
      state.appPolicy.proxy = previousProxy;
      state.appPolicy.direct = previousDirect;
      return;
    }
    if (!(await refreshApps(true))) {
      state.appPolicy.proxy = previousProxy;
      state.appPolicy.direct = previousDirect;
    }
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

async function searchPackages(): Promise<void> {
  await withAction("search-packages", () => refreshPackages());
}

async function copyAppPolicyReport(): Promise<void> {
  appReportCopied.value = await copyText(formatAppPolicyFullReport({
    mode: state.appPolicy.mode,
    proxy: state.appPolicy.proxy,
    direct: state.appPolicy.direct,
    summary: policySummary.value
  }));
  state.output = appReportCopied.value ? t('应用策略完整快照已复制。') : t('剪贴板不可用，应用策略快照未复制。');
}

async function copyAppPolicySafeReport(): Promise<void> {
  safeReportCopied.value = await copyText(formatAppPolicySafeReport({
    mode: state.appPolicy.mode,
    proxy: state.appPolicy.proxy,
    direct: state.appPolicy.direct,
    summary: policySummary.value
  }));
  state.output = safeReportCopied.value ? t('应用策略隐私摘要已复制。') : t('剪贴板不可用，应用策略摘要未复制。');
}

function requestRemoveApp(pkg: string, target: AppTarget): void {
  pendingAppAction.value = {
    key: `remove-${target}-${pkg}`,
    command: `app remove ${pkg} ${target}`,
    message: t('确认从 {target} 名单移除 {pkg}？', { target: target, pkg: pkg }),
    plan: actionPlan({ type: "remove", target, packages: [pkg] }),
    run: () => removeApp(pkg, target)
  };
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
    <PageHeader :overline="t('应用策略')" :title="t('应用名单')">
      <div class="flex flex-wrap gap-2">
        <Button variant="outline" :loading="isRunning('refresh-apps')" @click="withAction('refresh-apps', () => refreshApps())"><RefreshCw :size="17" />{{ t('读取名单') }}</Button>
        <Button variant="outline" :loading="isRunning('search-packages')" @click="searchPackages"><ListFilter :size="17" />{{ t('刷新应用') }}</Button>
        <details class="config-action-menu">
          <summary>{{ t('更多') }}</summary>
          <div>
            <Button variant="outline" :loading="isRunning('copy-app-policy-report')" @click="withAction('copy-app-policy-report', copyAppPolicyReport)"><Copy :size="17" />{{ appReportCopied ? t('已复制快照') : t('复制完整快照') }}</Button>
            <Button variant="outline" :loading="isRunning('copy-app-policy-safe-report')" @click="withAction('copy-app-policy-safe-report', copyAppPolicySafeReport)"><Copy :size="17" />{{ safeReportCopied ? t('已复制摘要') : t('复制隐私摘要') }}</Button>
          </div>
        </details>
      </div>
    </PageHeader>
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
      <h3 class="text-base font-medium">{{ t('代理名单') }}</h3>
      <p class="text-sm leading-6 text-[var(--mn-ink-muted)]">{{ t('已选应用将强制走 proxy。') }}</p>
      <SearchField v-model="proxySearchQuery" :placeholder="t('搜索应用名称或包名')" />
      <div class="grid gap-px divide-y divide-[var(--mn-border)] overflow-auto">
        <div
          v-for="app in proxyListApps"
          :key="app.info.packageName"
          class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2 py-2 sm:grid-cols-[minmax(0,1fr)_auto_auto]"
        >
          <label class="flex min-w-0 cursor-pointer items-center gap-2">
            <input
              type="checkbox"
              class="size-4 shrink-0 accent-[var(--mn-cactus)]"
              :checked="app.inList"
              :aria-label="t('选择 {value}', { value: app.info.appLabel })"
              @change="toggleAppList(app.info.packageName, 'proxy')"
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
          <Button v-if="app.inList" size="sm" variant="outline" :loading="isRunning(`remove-proxy-${app.info.packageName}`)" @click="requestRemoveApp(app.info.packageName, 'proxy')">{{ t('移除') }}</Button>
          <span v-else class="text-xs text-[var(--mn-ink-faint)]">{{ t('勾选加入') }}</span>
        </div>
        <em v-if="!proxyListApps.length" class="mn-empty">{{ state.packages.length ? t('没有匹配的应用。') : t('暂无应用，点“刷新应用”读取。') }}</em>
      </div>
    </Card>

    <!-- Direct list box -->
    <Card class="grid gap-3">
      <h3 class="text-base font-medium">{{ t('MagicNet 内直连') }}</h3>
      <p class="text-sm leading-6 text-[var(--mn-ink-muted)]">{{ t('已选应用在 MagicNet 内直连，直连仍经过 MagicNet。') }}</p>
      <SearchField v-model="directSearchQuery" :placeholder="t('搜索应用名称或包名')" />
      <div class="grid gap-px divide-y divide-[var(--mn-border)] overflow-auto">
        <div
          v-for="app in directListApps"
          :key="app.info.packageName"
          class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2 py-2 sm:grid-cols-[minmax(0,1fr)_auto_auto]"
        >
          <label class="flex min-w-0 cursor-pointer items-center gap-2">
            <input
              type="checkbox"
              class="size-4 shrink-0 accent-[var(--mn-cactus)]"
              :checked="app.inList"
              :aria-label="t('选择 {value}', { value: app.info.appLabel })"
              @change="toggleAppList(app.info.packageName, 'direct')"
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
          <Button v-if="app.inList" size="sm" variant="outline" :loading="isRunning(`remove-direct-${app.info.packageName}`)" @click="requestRemoveApp(app.info.packageName, 'direct')">{{ t('移除') }}</Button>
          <span v-else class="text-xs text-[var(--mn-ink-faint)]">{{ t('勾选加入') }}</span>
        </div>
        <em v-if="!directListApps.length" class="mn-empty">{{ state.packages.length ? t('没有匹配的应用。') : t('暂无应用，点“刷新应用”读取。') }}</em>
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
