<script setup lang="ts">
import {
  Bug,
  CloudDownload,
  Github,
  LayoutDashboard,
  MessageCircle,
  Monitor,
  Moon,
  MoreHorizontal,
  RefreshCw,
  Router,
  ScrollText,
  Settings,
  Sun,
  X,
} from "lucide-vue-next";
import { computed, defineAsyncComponent, nextTick, onMounted, onUnmounted, ref, watch, type Component } from "vue";
import { t } from "@/i18n";
import { SETTINGS_ROUTES, type SettingsRoute } from "@/components/pages/settingsRoutes";
import LanguageSelect from "@/components/LanguageSelect.vue";
import { MAGICNET_LOGO_URL } from "@/branding";
import IssueReporterDialog from "@/components/IssueReporterDialog.vue";
import OnboardingDialog from "@/components/OnboardingDialog.vue";
import OpenSourceSupportNote from "@/components/OpenSourceSupportNote.vue";
import Button from "@/components/ui/Button.vue";
import StatusDot from "@/components/ui/StatusDot.vue";
import { useMagicNet } from "@/composables/useMagicNet";
import { setPendingSubscriptionDraft } from "@/components/pages/subscriptionDraft";
import { useTheme } from "@/composables/useTheme";
import { useMobileKeyboard } from "@/composables/useMobileKeyboard";
import { restoreFocusAfterUpdate, trapFocusWithin } from "@/lib/focus";

type WorkspaceKey = "dashboard" | "nodes" | "subs" | "settings";
type OnboardingPreference = "dismissed" | "completed";

type WorkspaceDefinition = {
  key: WorkspaceKey;
  label: string;
  icon: Component;
};

const ONBOARDING_STORAGE_KEY = "magicnet.webui.onboarding.v1";

const workspaceLoaders: Record<WorkspaceKey, () => Promise<{ default: Component }>> = {
  dashboard: () => import("@/components/pages/DashboardPage.vue"),
  nodes: () => import("@/components/pages/NodesPage.vue"),
  subs: () => import("@/components/pages/SubscriptionsPage.vue"),
  settings: () => import("@/components/pages/SettingsHubPage.vue"),
};

/** Lazy workspace components keep inactive areas off the first-load critical path. */
const asyncPages = Object.fromEntries(
  (Object.keys(workspaceLoaders) as WorkspaceKey[]).map((key) => [
    key,
    defineAsyncComponent({
      loader: workspaceLoaders[key],
      delay: 80,
      timeout: 20000,
    }),
  ]),
) as Record<WorkspaceKey, Component>;

const workspaces: readonly WorkspaceDefinition[] = [
  { key: "dashboard", label: "仪表盘", icon: LayoutDashboard },
  { key: "nodes", label: "节点与路由", icon: Router },
  { key: "subs", label: "订阅", icon: CloudDownload },
  { key: "settings", label: "设置", icon: Settings },
];

/** Legacy tab keys (deep links + cross-page navigation) map onto the new model. */
const TAB_TARGETS: Record<string, { workspace: WorkspaceKey; settings?: SettingsRoute }> = {
  control: { workspace: "dashboard" },
  subs: { workspace: "subs" },
  proxy: { workspace: "nodes" },
  chain: { workspace: "nodes" },
  apps: { workspace: "settings", settings: "apps" },
  dns: { workspace: "settings", settings: "proxy" },
  stack: { workspace: "settings", settings: "proxy" },
  domain: { workspace: "settings", settings: "proxy" },
  block: { workspace: "settings", settings: "proxy" },
  config: { workspace: "settings", settings: "kernel" },
  webui: { workspace: "settings", settings: "kernel" },
  warp: { workspace: "settings", settings: "outbound" },
  tailscale: { workspace: "settings", settings: "outbound" },
  tools: { workspace: "settings", settings: "maint" },
  health: { workspace: "settings", settings: "maint" },
  terminal: { workspace: "settings", settings: "maint" },
  output: { workspace: "settings", settings: "logs" },
};

const {
  state,
  refreshAll,
  refreshStatus,
  refreshApps,
  refreshBlock,
  refreshSubs,
  refreshHealth,
  refreshMcp,
  refreshDns,
  refreshNetwork,
  refreshDomainForward,
  refreshWarp,
  createIssue,
  closeIssueReporter,
  submitIssue,
  openExternal,
  REPO,
  AUTHOR_WHISPER_URL,
} = useMagicNet();
const { preference: themePreference, label: themeLabel, cycleTheme } = useTheme();
const { keyboardOpen } = useMobileKeyboard();

type Location = { workspace: WorkspaceKey; settings: SettingsRoute | null };

function readLocation(): Location | null {
  if (typeof window === "undefined") return null;
  const raw = window.location.hash.replace(/^#\/?/, "");
  if (!raw) return null;
  const [head, sub] = raw.split("/");
  if (workspaces.some((item) => item.key === head)) {
    if (head === "settings") {
      const route = SETTINGS_ROUTES.includes(sub as SettingsRoute) ? (sub as SettingsRoute) : null;
      return { workspace: "settings", settings: route };
    }
    return { workspace: head as WorkspaceKey, settings: null };
  }
  const target = TAB_TARGETS[head];
  if (target) return { workspace: target.workspace, settings: target.settings ?? null };
  return null;
}

const initialLocation = readLocation();
const activeWorkspaceKey = ref<WorkspaceKey>(initialLocation?.workspace ?? "dashboard");
const settingsRoute = ref<SettingsRoute | null>(
  initialLocation?.workspace === "settings" ? initialLocation.settings : null,
);

function locationHash(): string {
  if (activeWorkspaceKey.value === "settings" && settingsRoute.value) {
    return `#/settings/${settingsRoute.value}`;
  }
  return `#/${activeWorkspaceKey.value}`;
}

function writeLocation(replace = false): void {
  if (typeof window === "undefined") return;
  const hash = locationHash();
  if (window.location.hash === hash) return;
  window.history[replace ? "replaceState" : "pushState"]({}, "", hash);
}

function warmWorkspace(workspace: WorkspaceKey): void {
  if (workspace === "dashboard") void refreshStatus();
  else if (workspace === "subs") void refreshSubs(true);
  // Settings detail pages refresh themselves from inside SettingsHubPage.
}

function selectWorkspace(workspace: WorkspaceKey): void {
  const changed = workspace !== activeWorkspaceKey.value;
  activeWorkspaceKey.value = workspace;
  // Tapping the settings tab always returns to its hub.
  settingsRoute.value = null;
  if (changed) warmWorkspace(workspace);
  void workspaceLoaders[workspace]();
  writeLocation();
}

function openSettings(route: SettingsRoute): void {
  activeWorkspaceKey.value = "settings";
  settingsRoute.value = route;
  writeLocation();
}

function closeSettings(): void {
  settingsRoute.value = null;
  writeLocation();
}

function gotoTab(key: string): void {
  const target = TAB_TARGETS[key];
  if (!target) return;
  activeWorkspaceKey.value = target.workspace;
  settingsRoute.value = target.workspace === "settings" ? (target.settings ?? null) : null;
  warmWorkspace(target.workspace);
  void workspaceLoaders[target.workspace]();
  writeLocation();
}

function syncFromLocation(): void {
  const loc = readLocation();
  if (!loc) return;
  activeWorkspaceKey.value = loc.workspace;
  settingsRoute.value = loc.workspace === "settings" ? loc.settings : null;
  warmWorkspace(loc.workspace);
  void workspaceLoaders[loc.workspace]();
}

const showUtilityMenu = ref(false);
const showOnboarding = ref(false);
const utilityDialog = ref<HTMLElement | null>(null);
const utilityMenuTrigger = ref<HTMLElement | null>(null);
const onboardingTrigger = ref<HTMLElement | null>(null);

const easterEggVisitors = [
  {
    name: "GPT-6",
    title: "GPT-6 到此一游",
    body: "少画几个框，多留一点位置给内容。",
  },
  {
    name: "SOL",
    title: "准备好你的太阳镜 😎",
    body: "SOL 到此一游 · 光太亮，别直视内核。",
  },
  {
    name: "Grok 4.5",
    title: "Grok 4.5 到此一游 ✨",
    body: "xAI 路过 MagicNet · 路由已嗅探，彩蛋已落盘。",
  },
  {
    name: "Kimi K3",
    title: "Kimi K3 到此一游 🌙",
    body: "月之暗面打卡成功 · 上下文很长，短签也行。",
  },
  {
    name: "Fable 5",
    title: "Fable 5 到此一游 📖",
    body: "Claude Fable 5 翻过这页 · 故事继续，彩蛋已签收。",
  },
] as const;

const easterEggVisible = ref(false);
const easterEggShownIndex = ref(0);
let easterEggNextIndex = 0;
const easterEggPayload = computed(
  () => easterEggVisitors[easterEggShownIndex.value] ?? easterEggVisitors[0],
);
let brandClickCount = 0;
let brandClickWindowStartedAt = 0;
let easterEggTimer: number | undefined;
let bodyOverflowBeforeDialog = "";

const activeWorkspace = computed(
  () => workspaces.find((item) => item.key === activeWorkspaceKey.value) ?? workspaces[0],
);
const activeComponent = computed(() => asyncPages[activeWorkspaceKey.value]);
const activeComponentProps = computed(() =>
  activeWorkspaceKey.value === "settings" ? { route: settingsRoute.value } : {},
);

const operationPanelVisible = ref(false);
let operationPanelTimer: number | undefined;
const operationPanelActive = computed(() => ["accepted", "queued", "running"].includes(state.operationCapture.phase));

watch(
  () => [state.operationCapture.sequence, state.operationCapture.phase] as const,
  ([, phase]) => {
    if (operationPanelTimer !== undefined) {
      window.clearTimeout(operationPanelTimer);
      operationPanelTimer = undefined;
    }
    if (phase === "accepted" || phase === "queued" || phase === "running") {
      operationPanelVisible.value = true;
      return;
    }
    if (phase === "done" || phase === "error") {
      operationPanelVisible.value = true;
      operationPanelTimer = window.setTimeout(() => {
        operationPanelVisible.value = false;
        operationPanelTimer = undefined;
      }, 1400);
    }
  },
);

function readOnboardingPreference(): OnboardingPreference | null {
  if (typeof window === "undefined") return null;
  try {
    const stored = window.localStorage.getItem(ONBOARDING_STORAGE_KEY);
    if (stored === "dismissed" || stored === "completed") return stored;
  } catch {
    return null;
  }
  return null;
}

function persistOnboardingPreference(value: OnboardingPreference): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(ONBOARDING_STORAGE_KEY, value);
  } catch {
    // Ignore storage failures; the dialog remains usable for the current session.
  }
}

function restoreOnboardingTriggerFocus(): void {
  const trigger = onboardingTrigger.value;
  onboardingTrigger.value = null;
  restoreFocusAfterUpdate(trigger);
}

function launchOnboarding(trigger: HTMLElement | null = null): void {
  onboardingTrigger.value = trigger;
  showOnboarding.value = true;
}

async function requestOnboarding(event?: MouseEvent): Promise<void> {
  const trigger = event?.currentTarget instanceof HTMLElement ? event.currentTarget : null;
  if (showUtilityMenu.value) {
    closeUtilityMenu(false);
    await nextTick();
    launchOnboarding(utilityMenuTrigger.value);
    return;
  }
  launchOnboarding(trigger);
}

function closeOnboarding(preference: OnboardingPreference = "dismissed"): void {
  if (!showOnboarding.value) return;
  persistOnboardingPreference(preference);
  showOnboarding.value = false;
  restoreOnboardingTriggerFocus();
}

function handleOnboardingSubmit(value: string): void {
  setPendingSubscriptionDraft(value);
  closeOnboarding("completed");
  gotoTab("subs");
}

async function requestIssue(): Promise<void> {
  if (showUtilityMenu.value) closeUtilityMenu(false);
  await createIssue();
}

async function openUtilityMenu(event: MouseEvent): Promise<void> {
  if (event.currentTarget instanceof HTMLElement) {
    utilityMenuTrigger.value = event.currentTarget;
  }
  bodyOverflowBeforeDialog = document.body.style.overflow;
  document.body.style.overflow = "hidden";
  showUtilityMenu.value = true;
  await nextTick();
  const initial = utilityDialog.value?.querySelector<HTMLElement>("[data-dialog-initial-focus]");
  (initial || utilityDialog.value)?.focus();
}

function closeUtilityMenu(restoreFocus = true): void {
  if (!showUtilityMenu.value) return;
  showUtilityMenu.value = false;
  document.body.style.overflow = bodyOverflowBeforeDialog;
  const trigger = utilityMenuTrigger.value;
  if (restoreFocus) restoreFocusAfterUpdate(trigger);
}

function trapUtilityMenuFocus(event: KeyboardEvent): void {
  if (!showUtilityMenu.value) return;
  trapFocusWithin(event, utilityDialog.value);
}

function closeEasterEgg(): void {
  easterEggVisible.value = false;
  if (easterEggTimer !== undefined) window.clearTimeout(easterEggTimer);
  if (operationPanelTimer !== undefined) window.clearTimeout(operationPanelTimer);
  easterEggTimer = undefined;
}

function handleBrandMarkClick(): void {
  const now = Date.now();
  if (now - brandClickWindowStartedAt > 2400) brandClickCount = 0;
  if (brandClickCount === 0) brandClickWindowStartedAt = now;
  brandClickCount += 1;
  if (brandClickCount < 5) return;

  brandClickCount = 0;
  easterEggShownIndex.value = easterEggNextIndex % easterEggVisitors.length;
  easterEggNextIndex = (easterEggShownIndex.value + 1) % easterEggVisitors.length;
  easterEggVisible.value = true;
  if (easterEggTimer !== undefined) window.clearTimeout(easterEggTimer);
  easterEggTimer = window.setTimeout(closeEasterEgg, 5200);
}

function dismissActionMenus(event: PointerEvent): void {
  if (!(event.target instanceof Node)) return;
  for (const menu of document.querySelectorAll<HTMLDetailsElement>(".config-action-menu[open]")) {
    if (!menu.contains(event.target)) menu.open = false;
  }
}

function handleEscape(event: KeyboardEvent): void {
  if (event.key === "Tab") {
    trapUtilityMenuFocus(event);
    return;
  }
  if (event.key !== "Escape") return;
  if (showUtilityMenu.value) {
    event.preventDefault();
    closeUtilityMenu();
    return;
  }
  const menu = document.querySelector<HTMLDetailsElement>(".config-action-menu[open]");
  if (menu) {
    event.preventDefault();
    menu.open = false;
    menu.querySelector("summary")?.focus();
    return;
  }
  closeEasterEgg();
}

function prefetchWorkspace(workspace: WorkspaceKey): void {
  void workspaceLoaders[workspace]();
}

onMounted(() => {
  void refreshStatus();
  warmWorkspace(activeWorkspaceKey.value);
  document.addEventListener("keydown", handleEscape);
  document.addEventListener("pointerdown", dismissActionMenus);
  window.addEventListener("popstate", syncFromLocation);
  window.addEventListener("hashchange", syncFromLocation);
  writeLocation(true);
  void workspaceLoaders[activeWorkspaceKey.value]();
  if (state.hasKsu && !readOnboardingPreference()) {
    void nextTick(() => {
      if (!showOnboarding.value) launchOnboarding();
    });
  }
  const warm = () => {
    void workspaceLoaders.nodes();
    void workspaceLoaders.subs();
    void workspaceLoaders.settings();
  };
  if (typeof requestIdleCallback === "function") requestIdleCallback(warm, { timeout: 2500 });
  else window.setTimeout(warm, 800);
});

onUnmounted(() => {
  document.removeEventListener("keydown", handleEscape);
  document.removeEventListener("pointerdown", dismissActionMenus);
  window.removeEventListener("popstate", syncFromLocation);
  window.removeEventListener("hashchange", syncFromLocation);
  if (easterEggTimer !== undefined) window.clearTimeout(easterEggTimer);
  if (showUtilityMenu.value) document.body.style.overflow = bodyOverflowBeforeDialog;
});
</script>

<template>
  <div class="mn-shell" :class="{ 'mn-keyboard-open': keyboardOpen }">
    <header class="mn-command-bar">
      <div class="mn-brand-lockup">
        <button
          class="mn-brand-mark brand-mark"
          type="button"
          :aria-label="t('MagicNet 品牌标记')"
          title="MagicNet"
          @click="handleBrandMarkClick"
        >
          <img
            :src="MAGICNET_LOGO_URL"
            alt=""
            width="42"
            height="42"
            decoding="async"
          />
        </button>
        <div class="mn-brand-copy">
          <h1>MagicNet</h1>
        </div>
      </div>

      <div class="mn-global-actions">
        <LanguageSelect class="mn-language-header" />
        <Button
          variant="ghost"
          size="icon"
          :aria-label="t('外观主题：{theme}，点击切换', { theme: themeLabel })"
          :title="t('外观：{theme}（亮色 → 暗色 → 跟随系统）', { theme: themeLabel })"
          @click="cycleTheme"
        >
          <Sun v-if="themePreference === 'light'" :size="17" aria-hidden="true" />
          <Moon v-else-if="themePreference === 'dark'" :size="17" aria-hidden="true" />
          <Monitor v-else :size="17" aria-hidden="true" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          :loading="state.task === '刷新面板'"
          :aria-label="t('刷新面板')"
          :title="t('刷新面板')"
          @click="refreshAll"
        >
          <RefreshCw :size="17" aria-hidden="true" />
        </Button>
        <Button
          class="mn-desktop-action"
          variant="outline"
          size="sm"
          :aria-label="t('打开新手引导')"
          @click="requestOnboarding"
        >
          <ScrollText :size="16" aria-hidden="true" />{{ t('引导') }}
        </Button>
        <Button
          class="mn-desktop-action"
          variant="outline"
          size="sm"
          :loading="state.task === '创建 GitHub issue'"
          :aria-label="t('创建 GitHub Issue')"
          @click="createIssue"
        >
          <Bug :size="16" aria-hidden="true" />{{ t('反馈') }}
        </Button>
        <Button
          class="mn-desktop-action"
          variant="outline"
          size="sm"
          :aria-label="t('打开 GitHub')"
          @click="openExternal(REPO, 'GitHub')"
        >
          <Github :size="16" aria-hidden="true" />GitHub
        </Button>
        <Button
          class="mn-more-action"
          variant="ghost"
          size="icon"
          :aria-label="t('打开系统工具')"
          aria-haspopup="dialog"
          :aria-expanded="showUtilityMenu"
          @click="openUtilityMenu"
        >
          <MoreHorizontal :size="18" aria-hidden="true" />
        </Button>
      </div>
    </header>

    <Transition name="operation-panel">
      <div v-if="operationPanelVisible" class="mn-operation-overlay">
        <section class="mn-operation-glass" :data-phase="state.operationCapture.phase" role="status" aria-live="polite" aria-atomic="true">
          <div class="mn-operation-heading">
            <StatusDot :tone="operationPanelActive ? 'current' : state.operationCapture.phase === 'error' ? 'stop' : 'ok'" />
            <strong>{{ operationPanelActive ? t('正在执行') : state.operationCapture.phase === 'error' ? t('操作失败') : t('操作完成') }}</strong>
            <span v-if="state.task" class="mn-operation-task">{{ t(state.task) }}</span>
          </div>
          <code class="mn-operation-command">$ {{ state.operationCapture.command }}</code>
          <pre class="mn-operation-output">{{ state.operationCapture.output }}</pre>
          <Button v-if="state.backgroundTask.log" variant="ghost" size="sm" @click="gotoTab('output')">{{ t('查看输出') }}</Button>
        </section>
      </div>
    </Transition>

    <div class="mn-workspace-frame">
      <aside class="desktop-rail" :aria-label="t('MagicNet 工作区')">
        <nav :aria-label="t('全部页面')">
          <button
            v-for="workspace in workspaces"
            :key="workspace.key"
            :class="activeWorkspaceKey === workspace.key ? 'mn-nav-active' : 'mn-nav-idle'"
            type="button"
            :aria-current="activeWorkspaceKey === workspace.key ? 'page' : undefined"
            @pointerenter="prefetchWorkspace(workspace.key)"
            @focus="prefetchWorkspace(workspace.key)"
            @click="selectWorkspace(workspace.key)"
          >
            <component :is="workspace.icon" :size="16" aria-hidden="true" />
            <span>{{ t(workspace.label) }}</span>
          </button>
        </nav>
      </aside>

      <main class="mn-workspace-main">
        <!-- KeepAlive preserves form state across the four workspaces. -->
        <section class="page-surface" :data-page="activeWorkspaceKey">
          <Suspense>
            <KeepAlive :max="8">
              <component
                :is="activeComponent"
                v-bind="activeComponentProps"
                @goto-output="gotoTab('output')"
                @goto-tab="gotoTab"
                @open="openSettings"
                @back="closeSettings"
              />
            </KeepAlive>
            <template #fallback>
              <div class="mn-loading-panel" role="status">{{ t('正在加载…') }}</div>
            </template>
          </Suspense>
        </section>
      </main>
    </div>

    <nav v-show="!keyboardOpen" class="mobile-nav" :aria-label="t('MagicNet 移动导航')">
      <button
        v-for="workspace in workspaces"
        :key="workspace.key"
        :data-workspace="workspace.key"
        :class="activeWorkspaceKey === workspace.key ? 'mn-nav-active' : 'mn-nav-idle'"
        type="button"
        :aria-current="activeWorkspaceKey === workspace.key ? 'page' : undefined"
        @click="selectWorkspace(workspace.key)"
      >
        <component :is="workspace.icon" :size="19" aria-hidden="true" />
        <span>{{ t(workspace.label) }}</span>
      </button>
    </nav>

    <Transition name="sheet">
      <div v-if="showUtilityMenu" class="mn-sheet-layer">
        <button class="mn-overlay" type="button" :aria-label="t('关闭系统工具')" @click="closeUtilityMenu()" />
        <div
          ref="utilityDialog"
          class="mn-utility-sheet"
          role="dialog"
          aria-modal="true"
          :aria-label="t('系统工具')"
          tabindex="-1"
        >
          <div class="mn-sheet-header">
            <div>
              <h2>{{ t('系统工具') }}</h2>
            </div>
            <Button data-dialog-initial-focus variant="ghost" size="icon" :aria-label="t('关闭系统工具')" @click="closeUtilityMenu()">
              <X :size="18" aria-hidden="true" />
            </Button>
          </div>
          <div class="mn-language-setting">
            <span>{{ t('语言') }}</span>
            <LanguageSelect />
          </div>
          <div class="mn-utility-grid">
            <Button variant="outline" @click="requestOnboarding">
              <ScrollText :size="18" aria-hidden="true" />{{ t('新手引导') }}
            </Button>
            <Button variant="outline" :loading="state.task === '创建 GitHub issue'" @click="requestIssue">
              <Bug :size="18" aria-hidden="true" />{{ t('反馈问题') }}
            </Button>
            <Button variant="outline" @click="openExternal(AUTHOR_WHISPER_URL, '悄悄话')">
              <MessageCircle :size="18" aria-hidden="true" />{{ t('悄悄话') }}
            </Button>
            <Button variant="outline" @click="openExternal(REPO, 'GitHub')">
              <Github :size="18" aria-hidden="true" />GitHub
            </Button>
          </div>
          <OpenSourceSupportNote />
        </div>
      </div>
    </Transition>

    <OnboardingDialog
      v-if="showOnboarding"
      @dismiss="closeOnboarding()"
      @submit="handleOnboardingSubmit"
    />

    <IssueReporterDialog
      v-if="state.issueReporter.open"
      :loading="state.task === '创建 GitHub issue'"
      @cancel="closeIssueReporter"
      @confirm="submitIssue"
    />

    <Transition name="toast">
      <aside v-if="easterEggVisible" class="mn-easter-egg" role="status" aria-live="polite">
        <div class="mn-easter-mark">
          <img :src="MAGICNET_LOGO_URL" alt="" width="38" height="38" decoding="async" />
        </div>
        <div class="mn-easter-copy">
          <strong>{{ t(easterEggPayload.title) }}</strong>
          <p>{{ t(easterEggPayload.body) }}</p>
          <p class="mn-visitor-line">
            <span>{{ t('来访') }}</span>
            <template v-for="(visitor, index) in easterEggVisitors" :key="visitor.name">
              <span aria-hidden="true">/</span>
              <span :class="index === easterEggShownIndex ? 'is-current' : undefined">{{ visitor.name }}</span>
            </template>
          </p>
        </div>
        <button type="button" :aria-label="t('关闭彩蛋')" @click="closeEasterEgg">
          <X :size="16" aria-hidden="true" />
        </button>
      </aside>
    </Transition>
  </div>
</template>
