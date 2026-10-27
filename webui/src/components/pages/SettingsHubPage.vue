<script setup lang="ts">
import {
  ChevronLeft,
  ChevronRight,
  Cpu,
  LayoutGrid,
  Router,
  ScrollText,
  Waypoints,
  Wrench,
} from "lucide-vue-next";
import { computed, defineAsyncComponent, watch, type Component } from "vue";
import { t } from "@/i18n";
import { useMagicNet } from "@/composables/useMagicNet";
import type { SettingsRoute } from "./settingsRoutes";

const props = defineProps<{ route: SettingsRoute | null }>();
const emit = defineEmits<{
  (e: "open", route: SettingsRoute): void;
  (e: "back"): void;
}>();

type SettingsEntry = {
  key: SettingsRoute;
  title: string;
  subtitle: string;
  icon: Component;
};

type SettingsGroup = {
  label: string;
  items: SettingsEntry[];
};

const groups: readonly SettingsGroup[] = [
  {
    label: "代理",
    items: [
      { key: "proxy", title: "代理设置", subtitle: "协议栈、DNS、域名转发与分流规则", icon: Router },
      { key: "kernel", title: "内核设置", subtitle: "sing-box 配置文件与管理面板", icon: Cpu },
      { key: "apps", title: "应用分流", subtitle: "选择哪些应用需要走代理", icon: LayoutGrid },
      { key: "outbound", title: "外部出站", subtitle: "WARP 与 Tailscale 出站", icon: Waypoints },
      { key: "builtin-outbound", title: "内置出站", subtitle: "查看 MagicNet 内部策略组与引用状态", icon: Router },
    ],
  },
  {
    label: "系统",
    items: [
      { key: "service", title: "服务管理", subtitle: "应用配置、自修复、API 检查等", icon: Cpu },
      { key: "logs", title: "日志", subtitle: "查看最近的命令输出", icon: ScrollText },
      { key: "maint", title: "维护", subtitle: "健康检查、终端与维护操作", icon: Wrench },
    ],
  },
];

const detailPages: Record<SettingsRoute, Component> = {
  proxy: defineAsyncComponent(() => import("./settings/ProxySettingsPage.vue")),
  kernel: defineAsyncComponent(() => import("./settings/KernelSettingsPage.vue")),
  apps: defineAsyncComponent(() => import("./AppsPage.vue")),
  outbound: defineAsyncComponent(() => import("./settings/OutboundPage.vue")),
  "builtin-outbound": defineAsyncComponent(() => import("./settings/BuiltinOutboundPage.vue")),
  logs: defineAsyncComponent(() => import("./OutputPage.vue")),
  maint: defineAsyncComponent(() => import("./settings/MaintenancePage.vue")),
  service: defineAsyncComponent(() => import("./settings/ServiceManagementPage.vue")),
};

const detailTitles: Record<SettingsRoute, string> = {
  proxy: "代理设置",
  kernel: "内核设置",
  apps: "应用分流",
  outbound: "外部出站",
  "builtin-outbound": "内置出站",
  logs: "日志",
  maint: "维护",
  service: "服务管理",
};

const activeComponent = computed(() => (props.route ? detailPages[props.route] : null));
const activeTitle = computed(() => (props.route ? t(detailTitles[props.route]) : ""));

const {
  refreshApps,
  refreshBlock,
  refreshDns,
  refreshDomainForward,
  refreshNetwork,
  refreshWarp,
  refreshMcp,
  refreshHealth,
} = useMagicNet();

function warmDetail(route: SettingsRoute | null): void {
  if (route === "proxy") {
    void refreshNetwork(true);
    void refreshDns(true);
    void refreshDomainForward(true);
    void refreshBlock(true);
  } else if (route === "apps") {
    void refreshApps(true);
  } else if (route === "outbound") {
    void refreshWarp(true);
  } else if (route === "kernel") {
    void refreshMcp(true);
  } else if (route === "maint") {
    void refreshMcp(true);
    void refreshHealth(true);
  }
}

watch(
  () => props.route,
  (route) => warmDetail(route),
  { immediate: true },
);
</script>

<template>
  <div class="mn-settings-hub min-w-0">
    <template v-if="!route">
      <h2 class="mn-hub-title">{{ t("设置") }}</h2>
      <div class="mn-hub-groups">
        <section v-for="group in groups" :key="group.label">
          <p class="mn-hub-section-label">{{ t(group.label) }}</p>
          <div class="mn-hub-card">
            <button
              v-for="item in group.items"
              :key="item.key"
              type="button"
              class="mn-hub-row"
              @click="emit('open', item.key)"
            >
              <span class="mn-hub-row-icon">
                <component :is="item.icon" :size="20" aria-hidden="true" />
              </span>
              <span class="mn-hub-row-body">
                <strong>{{ t(item.title) }}</strong>
                <span>{{ t(item.subtitle) }}</span>
              </span>
              <ChevronRight class="mn-hub-row-chevron" :size="18" aria-hidden="true" />
            </button>
          </div>
        </section>
      </div>
    </template>

    <template v-else>
      <div class="mn-detail-head">
        <button
          type="button"
          class="mn-detail-back"
          :aria-label="t('返回设置')"
          @click="emit('back')"
        >
          <ChevronLeft :size="20" aria-hidden="true" />
        </button>
        <h2>{{ activeTitle }}</h2>
      </div>
      <Suspense>
        <KeepAlive :max="6">
          <component :is="activeComponent" />
        </KeepAlive>
        <template #fallback>
          <div class="mn-loading-panel" role="status">{{ t("正在加载…") }}</div>
        </template>
      </Suspense>
    </template>
  </div>
</template>
