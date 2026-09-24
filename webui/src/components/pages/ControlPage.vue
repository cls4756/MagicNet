<script setup lang="ts">
import { t } from "@/i18n";
import {
  DownloadCloud,
  Cpu,
  HelpCircle,
  MemoryStick,
  Pencil,
  Plus,
  Power,
  Radar,
  RotateCcw,
  Share2,
  Wifi,
} from "lucide-vue-next";
import { computed, nextTick, onDeactivated, onMounted, ref, watch } from "vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import Card from "@/components/ui/Card.vue";
import CardHeading from "@/components/ui/CardHeading.vue";
import ConfirmPanel from "@/components/ui/ConfirmPanel.vue";
import Input from "@/components/ui/Input.vue";
import StatTile from "@/components/ui/StatTile.vue";
import StatusDot from "@/components/ui/StatusDot.vue";
import {
  applyTransparentModeAction,
  type ControlDangerAction,
  setTransparentModeAction,
  singBoxToggleAction,
} from "@/components/pages/controlDangerActions";
import { useActionLock } from "@/composables/useActionLock";
import { useMagicNet } from "@/composables/useMagicNet";
import { restoreFocusAfterUpdate, trapFocusWithin } from "@/lib/focus";
import type { TransparentMode } from "@/types";
import { execFailed } from "@/utils";
import {
  buildControlRuntimeInsight,
  controlInsightTone,
  controlRuntimeBusy,
} from "./controlRuntimeInsight";

const {
  state,
  runCli,
  startBackgroundCli,
  refreshAll,
  refreshStatus,
  refreshWifiPolicy,
  shellQuote,
} = useMagicNet();
const { isRunning, withAction } = useActionLock();

const emit = defineEmits<{
  (e: "goto-tab", tab: "health" | "output"): void;
}>();
type HotspotPolicyPhase = "loading" | "ready" | "error";
type SingBoxStatusPresentation = {
  label: string;
  tone: "neutral" | "success" | "warning";
  dotClass: string;
};

const pendingDangerAction = ref<ControlDangerAction | null>(null);
const dangerConfirmCard = ref<HTMLElement | null>(null);
const wifiSsidInput = ref("");
const wifiBssidInput = ref("");
const editingWifiEntry = ref<{ kind: "ssid" | "bssid"; value: string } | null>(null);
const editingWifiValue = ref("");
const hotspotProxyEnabled = ref(false);
const showHotspotHelp = ref(false);
const hotspotRouteStatus = ref("");
const hotspotForwardingLabel = computed(() => {
  switch (hotspotRouteStatus.value) {
    case "ready": return t("热点转发规则已就绪");
    case "waiting-for-hotspot": return t("已启用，等待热点开启");
    case "degraded": return t("已启用，但转发规则异常");
    case "shared-tc-unverified": return t("eBPF 共享转发待核实");
    default: return t("已设置，转发状态未确认");
  }
});
const hotspotPolicyPhase = ref<HotspotPolicyPhase>("loading");
const hotspotPolicyError = ref("");
let dangerActionTrigger: HTMLElement | null = null;

const pendingDangerMessage = computed(
  () => pendingDangerAction.value?.message ?? "",
);
const singBoxStatus = computed<SingBoxStatusPresentation>(() => {
  const rawState = state.runtime.singBoxState;
  if (rawState === "sing-box") {
    return {
      label: state.runtime.serviceReady === false ? t("服务未就绪") : t("运行中"),
      tone: state.runtime.serviceReady === false ? "warning" : "success",
      dotClass: "bg-[var(--mn-cactus)]",
    };
  }
  if (rawState === "stopped") {
    return {
      label: t("已停止"),
      tone: "warning",
      dotClass: "bg-[var(--mn-oat)]",
    };
  }
  return {
    label: !rawState || rawState === "unknown" ? t("状态未知") : rawState,
    tone: "neutral",
    dotClass: "bg-[var(--mn-ink-faint)]",
  };
});
const runtimeInsight = computed(() =>
  buildControlRuntimeInsight({
    hasKsu: state.hasKsu,
    phase: state.phase,
    queueDepth: state.queueDepth,
    backgroundStatus: state.backgroundTask.status,
    runtime: state.runtime,
    output: state.output,
  }),
);
const runtimeBusy = computed(() =>
  controlRuntimeBusy(state.phase, state.queueDepth, state.backgroundTask.status),
);
const missingNodeCache = computed(() =>
  /No cached sing-box nodes found|run cli sub update sing-box/i.test(
    state.output,
  ),
);

const showRuntimeNotice = computed(() => state.hasKsu && (
  missingNodeCache.value || state.phase === "error" ||
  (runtimeInsight.value.status !== "ok" && state.runtime.singBoxState !== "stopped")
));

const controlTitle = computed(() => {
  if (!state.hasKsu) return t("未连接设备");
  if (state.runtime.singBoxState === "sing-box")
    return state.runtime.serviceReady === false ? t("服务未就绪") : t("运行中");
  if (state.runtime.singBoxState === "stopped") return t("已停止");
  return t("状态未知");
});

const controlTitleClass = computed(() => {
  if (!state.hasKsu) return "text-[var(--mn-ink-muted)] font-medium";
  if (state.runtime.singBoxState === "sing-box") {
    return state.runtime.serviceReady === false
      ? "text-[var(--mn-warning)] font-medium"
      : "text-[var(--mn-success)] font-semibold";
  }
  if (state.runtime.singBoxState === "stopped") return "text-[var(--mn-danger)] font-semibold";
  return "text-[var(--mn-ink-muted)] font-medium";
});

const transparentModeLabel = computed(() => {
  if (state.runtime.transparentMode === "tun") return "TUN";
  if (state.runtime.transparentMode === "ebpf") return "eBPF";
  return t("状态未知");
});
const transparentEffectiveLabel = computed(() => {
  const effective = state.runtime.transparentEffectiveMode;
  if (effective === "tun") return "TUN · magicnet0";
  if (effective === "local") return "eBPF local";
  if (effective === "hybrid") return "eBPF hybrid";
  return t("状态未知");
});
const transparentDescription = computed(() => {
  if (state.runtime.transparentMode === "unknown") {
    return t("无法读取透明代理状态；当前模式不会按 TUN 或 eBPF 猜测。");
  }
  if (state.runtime.transparentMode === "tun") {
    return t("sing-box TUN 通过 magicnet0 接管本机流量。");
  }
  if (state.runtime.transparentEffectiveMode === "hybrid") {
    return t("eBPF local 已接管本机流量，shared TC 使用已确认的下游接口。");
  }
  if (state.runtime.transparentSharedTc === "pending") {
    return t("eBPF local 已配置；尚无已确认下游接口，shared TC 保持 pending。");
  }
  return t("eBPF 使用 cgroup 接管本机流量；shared 状态以运行时报告为准。");
});
const transparentTransitionTone = computed<"neutral" | "success" | "warning" | "danger">(() => {
  if (state.runtime.transparentTransition === "rollback") return "danger";
  if (state.runtime.transparentTransition === "pending") return "warning";
  if (state.runtime.transparentTransition === "stable") return "success";
  return "neutral";
});
const transparentSwitchBusy = computed(() =>
  runtimeBusy.value ||
  isRunning("transparent-set-tun") ||
  isRunning("transparent-set-ebpf") ||
  isRunning("transparent-apply"),
);
const sharedInterfacesLabel = computed(() =>
  state.runtime.transparentSharedInterfaces.join(", ") ||
    (state.runtime.transparentSharedInterfaceCount === null ? t("状态未知")
      : String(state.runtime.transparentSharedInterfaceCount)),
);

const wifiPolicyModes = [
  { value: "blacklist", label: "指定网络直连" },
  { value: "whitelist", label: "指定网络代理" },
] as const;

async function toggleSingBox(event: MouseEvent): Promise<void> {
  const running = state.runtime.singBoxState === "sing-box";
  requestDangerAction(singBoxToggleAction(running), event.currentTarget);
}

async function runAction(
  key: string,
  args: string,
  label: string,
  background = false,
): Promise<void> {
  await withAction(key, async () => {
    if (background) {
      const launch = await startBackgroundCli(args, label);
      if (execFailed(launch)) return;
      window.setTimeout(() => void refreshStatus(), 1200);
    } else {
      const output = await runCli(args, label);
      if (execFailed(output)) {
        if (args.startsWith("transparent set ")) await refreshStatus();
        return;
      }
      await refreshAll();
    }
  });
}

function requestTransparentMode(mode: TransparentMode, event: MouseEvent): void {
  if (mode === state.runtime.transparentMode || transparentSwitchBusy.value) return;
  requestDangerAction(
    setTransparentModeAction(mode, state.runtime.transparentMode),
    event.currentTarget,
  );
}

async function rebuildNodeCache(): Promise<void> {
  await withAction("rebuild-node-cache", async () => {
    // The background follower refreshes subscriptions and service status.
    await startBackgroundCli("sub update sing-box", t("重建 sing-box 节点缓存"));
  });
}

function restoreDangerActionFocus(): void {
  const trigger = dangerActionTrigger;
  dangerActionTrigger = null;
  restoreFocusAfterUpdate(trigger);
}

function requestDangerAction(
  action: ControlDangerAction,
  trigger: EventTarget | null = document.activeElement,
): void {
  dangerActionTrigger = trigger instanceof HTMLElement ? trigger : null;
  pendingDangerAction.value = action;
  void nextTick(() => {
    const reduceMotion =
      window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
    dangerConfirmCard.value?.scrollIntoView({
      block: "nearest",
      behavior: reduceMotion ? "auto" : "smooth",
    });
    dangerConfirmCard.value
      ?.querySelector<HTMLButtonElement>("[data-danger-cancel]")
      ?.focus();
  });
}

function cancelDangerAction(): void {
  pendingDangerAction.value = null;
  restoreDangerActionFocus();
}

function handleDangerKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    cancelDangerAction();
    return;
  }
  trapFocusWithin(event, dangerConfirmCard.value);
}

watch(pendingDangerAction, (action, _previous, onCleanup) => {
  if (!action) return;
  const previousOverflow = document.body.style.overflow;
  document.body.style.overflow = "hidden";
  onCleanup(() => { document.body.style.overflow = previousOverflow; });
});

onDeactivated(() => {
  pendingDangerAction.value = null;
  dangerActionTrigger = null;
});

async function confirmDangerAction(): Promise<void> {
  const action = pendingDangerAction.value;
  if (!action) return;
  if (runtimeBusy.value) {
    state.output = t("后台任务未结束，已拒绝执行新的控制操作。");
    return;
  }
  pendingDangerAction.value = null;
  restoreDangerActionFocus();
  await runAction(action.key, action.args, action.label, action.background);
}

async function runWifiAction(
  key: string,
  args: string,
  label: string,
): Promise<void> {
  await withAction(key, async () => {
    const output = await runCli(args, label);
    if (execFailed(output)) return;
    await refreshWifiPolicy(true);
  });
}

async function toggleWifiPolicy(): Promise<void> {
  await runWifiAction("wifi-toggle", `wifi ${!state.wifiPolicy.enabled ? "enable" : "disable"}`,
    !state.wifiPolicy.enabled ? t("启用 Wi-Fi 自动模式") : t("停用 Wi-Fi 自动模式"));
}

async function toggleWifiPolicyFromSwitch(event: Event): Promise<void> {
  const checkbox = event.currentTarget as HTMLInputElement;
  if (checkbox.checked === state.wifiPolicy.enabled) return;
  await toggleWifiPolicy();
}

async function refreshHotspotPolicy(): Promise<boolean> {
  hotspotPolicyPhase.value = "loading";
  hotspotPolicyError.value = "";
  const output = await runCli("hotspot status", t("读取热点代理策略"), true);
  if (execFailed(output)) {
    hotspotPolicyPhase.value = "error";
    hotspotPolicyError.value =
      t("MagicNet 没读到当前热点设置。设备设置没变，请重新读取。");
    state.output = t("读取热点代理策略失败：\n{output}", { output: output });
    return false;
  }
  const matched = output.match(/^enabled=([01])$/m);
  if (!matched) {
    hotspotPolicyPhase.value = "error";
    hotspotPolicyError.value =
      t("MagicNet 没认出设备返回的热点状态。设备设置没变，请重新读取。");
    state.output = t("读取热点代理策略失败：设备返回了无法解析的状态。");
    return false;
  }
  hotspotProxyEnabled.value = matched[1] === "1";
  hotspotRouteStatus.value = output.match(/^route_status=([a-z-]+)$/m)?.[1] ?? "";
  hotspotPolicyPhase.value = "ready";
  return true;
}

async function retryHotspotPolicy(): Promise<void> {
  await withAction("hotspot-policy-refresh", async () => {
    await refreshHotspotPolicy();
  });
}

async function toggleHotspotProxy(event: Event): Promise<void> {
  const checkbox = event.currentTarget as HTMLInputElement;
  if (hotspotPolicyPhase.value !== "ready") {
    checkbox.checked = hotspotProxyEnabled.value;
    return;
  }
  const previous = hotspotProxyEnabled.value;
  const enabled = checkbox.checked;
  hotspotProxyEnabled.value = enabled;
  await withAction("hotspot-proxy", async () => {
    const output = await runCli(
      `hotspot ${enabled ? "enable" : "disable"}`,
      enabled ? t("启用热点代理") : t("停用热点代理"),
    );
    if (execFailed(output)) {
      hotspotProxyEnabled.value = previous;
      return;
    }
    if (!(await refreshHotspotPolicy())) {
      hotspotProxyEnabled.value = previous;
    }
  });
}

async function setWifiPolicyMode(mode: "blacklist" | "whitelist"): Promise<void> {
  if (state.wifiPolicy.policyMode === mode) return;
  await runWifiAction(
    `wifi-mode-${mode}`,
    `wifi mode ${mode}`,
    t("切换 Wi-Fi {mode}", { mode: mode }),
  );
}

async function addWifiEntry(kind: "ssid" | "bssid"): Promise<void> {
  const input = kind === "ssid" ? wifiSsidInput : wifiBssidInput;
  const value = input.value.trim();
  if (!value) {
    state.output = kind === "ssid" ? t("请输入 SSID。") : t("请输入 BSSID。");
    return;
  }
  await runWifiAction(
    `wifi-add-${kind}`,
    `wifi add-${kind} ${shellQuote(value)}`,
    t("添加 Wi-Fi {kind}", { kind: kind.toUpperCase() }),
  );
  input.value = "";
}

async function removeWifiEntry(
  kind: "ssid" | "bssid",
  value: string,
): Promise<void> {
  await runWifiAction(
    `wifi-remove-${kind}-${value}`,
    `wifi remove-${kind} ${shellQuote(value)}`,
    t("移除 Wi-Fi {kind}", { kind: kind.toUpperCase() }),
  );
}

function beginEditWifiEntry(kind: "ssid" | "bssid", value: string): void {
  editingWifiEntry.value = { kind, value };
  editingWifiValue.value = value;
}

async function saveWifiEntryEdit(): Promise<void> {
  const entry = editingWifiEntry.value;
  const value = editingWifiValue.value.trim();
  if (!entry || !value || value === entry.value) {
    editingWifiEntry.value = null;
    return;
  }
  if ((entry.kind === "ssid" ? state.wifiPolicy.ssids : state.wifiPolicy.bssids).includes(value)) {
    state.output = t("该规则已存在。");
    return;
  }
  await withAction(`wifi-edit-${entry.kind}`, async () => {
    const removed = await runCli(`wifi remove-${entry.kind} ${shellQuote(entry.value)}`, t("更新 Wi-Fi 规则"));
    if (execFailed(removed)) return;
    const added = await runCli(`wifi add-${entry.kind} ${shellQuote(value)}`, t("更新 Wi-Fi 规则"));
    if (execFailed(added)) {
      await runCli(`wifi add-${entry.kind} ${shellQuote(entry.value)}`, t("恢复 Wi-Fi 规则"));
      await refreshWifiPolicy(true);
      return;
    }
    editingWifiEntry.value = null;
    await refreshWifiPolicy(true);
  });
}

async function addCurrentWifi(): Promise<void> {
  const { ssid, bssid } = state.wifiPolicy;
  if (!state.wifiPolicy.connected || (!ssid && !bssid)) {
    state.output = t("当前没有可添加的 Wi-Fi 信息。");
    return;
  }
  await withAction("wifi-add-current", async () => {
    if (ssid && !state.wifiPolicy.ssids.includes(ssid)) {
      const result = await runCli(`wifi add-ssid ${shellQuote(ssid)}`, t("添加当前 Wi-Fi"));
      if (execFailed(result)) return;
    }
    if (bssid && !state.wifiPolicy.bssids.includes(bssid)) {
      const result = await runCli(`wifi add-bssid ${shellQuote(bssid)}`, t("添加当前 Wi-Fi"));
      if (execFailed(result)) {
        await refreshWifiPolicy(true);
        return;
      }
    }
    await refreshWifiPolicy(true);
  });
}

onMounted(() => {
  void refreshHotspotPolicy();
});
</script>

<template>
  <div class="mn-control">
    <section class="mn-control-hero" :aria-label="t('服务概览')">
      <div class="mn-control-metrics" aria-live="polite">
        <article class="mn-control-metric mn-control-service">
          <div class="mn-control-metric-heading"><Power :size="18" /><span>{{ t("服务状态") }}</span></div>
          <div class="mn-control-service-row">
            <div class="mn-control-service-value">
              <span v-if="state.hasKsu" :class="['mn-control-dot', singBoxStatus.dotClass]" aria-hidden="true" />
              <strong :class="controlTitleClass">{{ controlTitle }}</strong>
              <span class="mn-control-muted">{{ state.hasKsu ? `sing-box · ${transparentModeLabel}` : t("请在模块管理器中打开") }}</span>
            </div>
            <Button class="mn-control-service-action" variant="outline" :disabled="runtimeBusy || !state.hasKsu"
              :loading="isRunning('toggle-sing-box')" @click="toggleSingBox">
              {{ state.runtime.singBoxState === 'sing-box' ? t("停止服务") : t("启动服务") }}
            </Button>
          </div>
        </article>
        <article class="mn-control-metric">
          <div class="mn-control-metric-heading"><Cpu :size="18" /><span>{{ t("CPU 占用") }}</span></div>
          <strong class="mn-control-metric-value">—</strong>
          <span class="mn-control-muted">{{ t("暂不可用") }}</span>
        </article>
        <article class="mn-control-metric">
          <div class="mn-control-metric-heading"><MemoryStick :size="18" /><span>{{ t("内存占用（RSS）") }}</span></div>
          <strong class="mn-control-metric-value">
            <template v-if="state.hasKsu && state.runtime.singBoxState === 'sing-box' && state.runtime.singBoxRssKib != null">
              {{ (state.runtime.singBoxRssKib / 1024).toFixed(1) }} <small>MiB</small>
            </template>
            <template v-else>—</template>
          </strong>
          <span class="mn-control-muted">{{ state.runtime.singBoxState === 'sing-box' && state.runtime.singBoxRssKib == null ? t("暂不可用") : "sing-box" }}</span>
        </article>
      </div>

      <details
        v-if="showRuntimeNotice"
        :open="missingNodeCache"
        class="mn-control-notice"
        :class="controlInsightTone(runtimeInsight.status)"
      >
        <summary><StatusDot tone="current" />{{ runtimeInsight.title }}</summary>
        <p>{{ runtimeInsight.detail }}</p>
        <Button v-if="missingNodeCache" variant="outline" :loading="isRunning('rebuild-node-cache')" @click="rebuildNodeCache">
          <DownloadCloud :size="17" />{{ t("更新订阅并重建节点") }} </Button>
        <Button v-else variant="outline" @click="emit('goto-tab', 'output')">{{ t("查看输出") }}</Button>
      </details>
    </section>

    <div class="mn-control-settings">
      <Card class="grid gap-5">
        <CardHeading :title="t('代理模式')">
          <Badge v-if="state.runtime.transparentMode === 'unknown'" tone="neutral">{{ t("未确认") }}</Badge>
        </CardHeading>

        <div
          role="group"
          :aria-label="t('选择透明代理模式')"
          class="grid grid-cols-2 gap-2"
        >
          <Button
            :variant="state.runtime.transparentMode === 'tun' ? 'default' : 'outline'"
            :disabled="!state.hasKsu || transparentSwitchBusy || state.runtime.transparentMode === 'tun'"
            :loading="isRunning('transparent-set-tun')"
            :aria-pressed="state.runtime.transparentMode === 'tun'"
            :class="state.runtime.transparentMode === 'tun' ? 'disabled:cursor-default disabled:opacity-100' : ''"
            @click="requestTransparentMode('tun', $event)"
          >
            TUN
          </Button>
          <Button
            :variant="state.runtime.transparentMode === 'ebpf' ? 'default' : 'outline'"
            :disabled="!state.hasKsu || transparentSwitchBusy || state.runtime.transparentMode === 'ebpf'"
            :loading="isRunning('transparent-set-ebpf')"
            :aria-pressed="state.runtime.transparentMode === 'ebpf'"
            :class="state.runtime.transparentMode === 'ebpf' ? 'disabled:cursor-default disabled:opacity-100' : ''"
            @click="requestTransparentMode('ebpf', $event)"
          >
            eBPF
          </Button>
        </div>

        <details class="mn-control-details">
          <summary>{{ t("运行详情") }}</summary>
          <p class="text-sm leading-6 text-[var(--mn-ink-muted)]">{{ transparentDescription }}</p>
          <Badge :tone="transparentTransitionTone">{{ state.runtime.transparentTransition }}</Badge>
        <dl
          aria-live="polite"
          class="grid gap-x-4 gap-y-2 rounded-[var(--mn-radius-md)] border border-[var(--mn-border)] bg-[var(--mn-surface-sunken)] p-3 text-xs sm:grid-cols-2"
        >
          <div class="min-w-0">
            <dt class="text-[var(--mn-ink-muted)]">configured</dt>
            <dd class="break-words font-mono text-[var(--mn-ink)]">{{ state.runtime.transparentMode }}</dd>
          </div>
          <div class="min-w-0">
            <dt class="text-[var(--mn-ink-muted)]">effective</dt>
            <dd class="break-words font-mono text-[var(--mn-ink)]">{{ transparentEffectiveLabel }}</dd>
          </div>
          <div class="min-w-0">
            <dt class="text-[var(--mn-ink-muted)]">local cgroup</dt>
            <dd class="break-words font-mono text-[var(--mn-ink)]">{{ state.runtime.transparentLocalCgroup }}</dd>
          </div>
          <div class="min-w-0">
            <dt class="text-[var(--mn-ink-muted)]">shared TC</dt>
            <dd class="break-words font-mono text-[var(--mn-ink)]">{{ state.runtime.transparentSharedTc }}</dd>
          </div>
          <div class="min-w-0 sm:col-span-2">
            <dt class="text-[var(--mn-ink-muted)]">shared interfaces</dt>
            <dd class="break-all font-mono text-[var(--mn-ink)]">{{ sharedInterfacesLabel }}</dd>
          </div>
          <div class="min-w-0">
            <dt class="text-[var(--mn-ink-muted)]">capability</dt>
            <dd class="break-words font-mono text-[var(--mn-ink)]">{{ state.runtime.transparentCapability }}</dd>
          </div>
          <div class="min-w-0">
            <dt class="text-[var(--mn-ink-muted)]">transition</dt>
            <dd class="break-words font-mono text-[var(--mn-ink)]">{{ state.runtime.transparentTransition }}</dd>
          </div>
        </dl>

        <Button
          variant="secondary"
          :disabled="!state.hasKsu || transparentSwitchBusy"
          :loading="isRunning('transparent-apply')"
          @click="requestDangerAction(applyTransparentModeAction(), $event.currentTarget)"
        >
          <Radar :size="17" />{{ t("重新应用当前模式") }} </Button>
        </details>
        <p v-if="state.runtime.transparentRecentError" role="alert" class="text-sm leading-6 text-[var(--mn-danger)]">
          {{ state.runtime.transparentRecentError }}
        </p>
      </Card>

      <Card class="grid gap-2" :aria-busy="hotspotPolicyPhase === 'loading'">
        <div class="mn-hotspot-row">
          <label class="mn-hotspot-switch">
          <input
            type="checkbox"
            role="switch"
            class="mn-hotspot-input"
            :checked="hotspotProxyEnabled"
            :disabled="!state.hasKsu || hotspotPolicyPhase !== 'ready' || runtimeBusy || isRunning('hotspot-proxy')"
            :aria-label="t('允许热点使用代理')"
            aria-describedby="hotspot-proxy-description hotspot-proxy-status"
            @change="toggleHotspotProxy"
          />
          <span class="min-w-0">
            <span class="mn-hotspot-label">
              <Share2 :size="17" />{{ t("热点代理") }}
              <Button variant="ghost" size="icon" class="mn-hotspot-help" :aria-label="t('热点代理帮助')" :aria-expanded="showHotspotHelp" @click="showHotspotHelp = !showHotspotHelp">
                <HelpCircle :size="17" aria-hidden="true" />
              </Button>
            </span>
            <span id="hotspot-proxy-status" class="mn-hotspot-state">
              {{ !state.hasKsu ? t("未连接设备") : hotspotPolicyPhase === 'loading' ? t("读取中") : hotspotPolicyPhase === 'error' ? t("读取失败") : hotspotProxyEnabled ? hotspotForwardingLabel : t("已关闭") }}
            </span>
          </span>
          <span class="mn-hotspot-track" aria-hidden="true" />
          </label>
        </div>
        <div v-if="showHotspotHelp" id="hotspot-proxy-description" class="mn-help-popover" role="dialog">
          <p>{{ t("热点设备使用 proxy 代理组；不勾选时统一走 direct。TUN 模式会关闭 Android 热点硬件加速，关闭代理后恢复原设置；eBPF 模式使用共享 TC。") }}</p>
          <Button v-if="state.hasKsu && hotspotProxyEnabled" variant="ghost" :disabled="runtimeBusy" @click="retryHotspotPolicy">{{ t("重新读取") }}</Button>
        </div>
        <div v-if="state.hasKsu && hotspotPolicyPhase === 'error'" class="mn-control-notice mn-tone-warn" role="alert">
          <p>{{ hotspotPolicyError }}</p>
          <Button variant="outline" :loading="isRunning('hotspot-policy-refresh')" @click="retryHotspotPolicy">
            <RotateCcw :size="16" />{{ t("重新读取") }} </Button>
        </div>
      </Card>

      <Card class="mn-wifi-card grid gap-3" :aria-busy="isRunning('wifi-toggle')">
        <CardHeading :title="t('Wi-Fi 自动切换')">
          <Badge :tone="state.wifiPolicy.observed && state.wifiPolicy.connected ? 'success' : 'neutral'">
            {{ !state.wifiPolicy.observed ? t("状态未知") : state.wifiPolicy.connected ? state.wifiPolicy.ssid || t("Wi-Fi 已连接") : t("未连接 Wi-Fi") }}
          </Badge>
          <label class="mn-hotspot-switch mn-wifi-switch">
            <input type="checkbox" role="switch" class="mn-hotspot-input" :checked="state.wifiPolicy.enabled"
              :disabled="runtimeBusy || !state.hasKsu || isRunning('wifi-toggle') || !state.wifiPolicy.observed"
              :aria-label="t('Wi-Fi 自动切换')" @change="toggleWifiPolicyFromSwitch" />
            <span class="mn-hotspot-track" aria-hidden="true" />
          </label>
        </CardHeading>

        <div v-if="state.wifiPolicy.enabled" class="flex flex-wrap gap-2" role="group" :aria-label="t('名单模式')">
          <Button
            v-for="mode in wifiPolicyModes"
            :key="mode.value"
            variant="outline"
            :aria-pressed="state.wifiPolicy.policyMode === mode.value"
            :disabled="!state.hasKsu || runtimeBusy || state.wifiPolicy.policyMode === mode.value"
            :class="[
              'mn-wifi-mode-btn',
              state.wifiPolicy.policyMode === mode.value
                ? 'mn-wifi-mode-btn-active'
                : 'mn-wifi-mode-btn-inactive',
            ]"
            @click="setWifiPolicyMode(mode.value)"
          >
            {{ t(mode.label) }}
          </Button>
        </div>

        <details v-if="state.wifiPolicy.enabled" class="mn-control-details mn-wifi-rules-details">
          <summary>{{ t("指定网络列表") }}</summary>
          <div class="grid gap-3">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <div><p class="text-xs text-[var(--mn-ink-muted)]">{{ state.wifiPolicy.policyMode === 'blacklist' ? t("列表中的网络直连，其余网络走代理。") : t("列表中的网络走代理，其余网络直连。") }}</p><p class="text-xs text-[var(--mn-ink-muted)]">{{ t("当前连接") }}：{{ state.wifiPolicy.ssid || t("未连接") }} · {{ state.wifiPolicy.bssid || '—' }}</p></div>
            <Button variant="secondary" :disabled="!state.wifiPolicy.connected || runtimeBusy" @click="addCurrentWifi"><Plus :size="16" />{{ t("添加当前 Wi-Fi") }}</Button>
          </div>
          <div class="grid gap-2 sm:grid-cols-2">
            <div class="flex gap-2"><Input v-model="wifiSsidInput" :aria-label="t('Wi-Fi 名称（SSID）')" :placeholder="t('输入 SSID')" @keyup.enter="addWifiEntry('ssid')" /><Button variant="secondary" :loading="isRunning('wifi-add-ssid')" @click="addWifiEntry('ssid')"><Plus :size="16" />SSID</Button></div>
            <div class="flex gap-2"><Input v-model="wifiBssidInput" aria-label="BSSID" :placeholder="t('输入 BSSID')" @keyup.enter="addWifiEntry('bssid')" /><Button variant="secondary" :loading="isRunning('wifi-add-bssid')" @click="addWifiEntry('bssid')"><Plus :size="16" />BSSID</Button></div>
          </div>
          <div class="overflow-x-auto">
            <table class="mn-wifi-table w-full text-sm">
              <thead><tr><th>{{ t("类型") }}</th><th>{{ t("规则值") }}</th><th>{{ t("操作") }}</th></tr></thead>
              <tbody>
                <tr v-for="entry in [...state.wifiPolicy.ssids.map(value => ({ kind: 'ssid' as const, value })), ...state.wifiPolicy.bssids.map(value => ({ kind: 'bssid' as const, value }))]" :key="`${entry.kind}:${entry.value}`">
                  <td>{{ entry.kind === 'ssid' ? 'SSID' : 'BSSID' }}</td>
                  <td>
                    <Input v-if="editingWifiEntry?.kind === entry.kind && editingWifiEntry.value === entry.value" v-model="editingWifiValue" @keyup.enter="saveWifiEntryEdit" />
                    <span v-else :class="entry.kind === 'bssid' ? 'font-mono' : ''">{{ entry.value }}</span>
                  </td>
                  <td class="whitespace-nowrap">
                    <template v-if="editingWifiEntry?.kind === entry.kind && editingWifiEntry.value === entry.value"><Button variant="secondary" @click="saveWifiEntryEdit">{{ t("保存") }}</Button><Button variant="ghost" @click="editingWifiEntry = null">{{ t("取消") }}</Button></template>
                    <template v-else><Button variant="ghost" :aria-label="t('编辑规则')" @click="beginEditWifiEntry(entry.kind, entry.value)"><Pencil :size="16" /></Button><Button variant="ghost" :aria-label="t('删除规则')" @click="removeWifiEntry(entry.kind, entry.value)">×</Button></template>
                  </td>
                </tr>
                <tr v-if="!state.wifiPolicy.ssids.length && !state.wifiPolicy.bssids.length"><td colspan="3" class="mn-empty text-center">{{ t("暂无 Wi-Fi 规则") }}</td></tr>
              </tbody>
            </table>
          </div>
          </div>
        </details>
      </Card>

    </div>

    <Teleport to="body">
      <Transition name="sheet">
        <div v-if="pendingDangerAction" class="mn-sheet-layer">
          <button class="mn-overlay" type="button" :aria-label="t('取消控制操作')" @click="cancelDangerAction" />
          <div ref="dangerConfirmCard" class="mn-utility-sheet mn-control-confirm" role="alertdialog" aria-modal="true" :aria-label="t('确认控制操作')" tabindex="-1" @keydown="handleDangerKeydown">
            <ConfirmPanel
              :title="t('确认操作')"
              :detail="pendingDangerMessage"
              :command="pendingDangerAction.args"
              :loading="isRunning(pendingDangerAction.key)"
              :confirm-label="t('继续执行')"
              confirm-variant="destructive"
              :auto-focus="false"
            >
              <template #actions>
                <Button data-danger-cancel variant="outline" @click="cancelDangerAction">{{ t("取消") }}</Button>
                <Button variant="destructive" :disabled="runtimeBusy" :loading="isRunning(pendingDangerAction.key)" @click="confirmDangerAction">{{ t("继续执行") }}</Button>
              </template>
            </ConfirmPanel>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
.mn-control {
  width: 100%;
  margin-inline: auto;
}

.mn-control-hero {
  padding: 4px 0 14px;
}

.mn-control-metrics {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
}

.mn-control-metric {
  display: grid;
  min-width: 0;
  gap: 7px;
  align-content: start;
  border: 1px solid var(--mn-border);
  border-radius: var(--mn-radius-lg);
  padding: 12px 14px;
  background: var(--mn-surface-raised);
  box-shadow: var(--mn-shadow-card);
}

.mn-control-service { grid-column: 1 / -1; }
.mn-control-metric-heading {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--mn-ink-muted);
  font-size: 13px;
}

.mn-control-metric-heading :deep(svg) { color: var(--mn-primary); }
.mn-control-service-row,
.mn-control-service-value {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 9px;
}
.mn-control-service-row { justify-content: space-between; }
.mn-control-service-value { flex-wrap: wrap; }
.mn-control-service-value strong { font-size: 17px; font-weight: 550; }
.mn-control-muted { color: var(--mn-ink-muted); font-size: 12px; }
.mn-control-metric-value {
  min-height: 27px;
  color: var(--mn-ink);
  font-size: 21px;
  font-weight: 500;
  font-variant-numeric: tabular-nums;
}
.mn-control-metric-value small { color: var(--mn-ink-muted); font-size: 12px; font-weight: 400; }

.mn-control-dot {
  width: 6px;
  height: 6px;
  flex: 0 0 6px;
  border-radius: 50%;
}

.mn-control-service-action {
  min-height: 34px;
  padding-inline: 10px;
  font-size: 12px;
}

.mn-control-settings {
  display: grid;
  gap: 8px;
}

.mn-control-settings :deep(.magic-card) {
  gap: 12px;
  padding: 14px;
}

.mn-control-notice {
  margin-top: 14px;
  border-radius: var(--mn-radius-md);
  padding: 12px 16px;
  font-size: 14px;
}

.mn-control-notice summary {
  display: flex;
  min-height: 40px;
  cursor: pointer;
  align-items: center;
  gap: 8px;
}

.mn-control-notice p,
.mn-control-notice button {
  margin-top: 10px;
}

.mn-control-details > summary {
  display: flex;
  min-height: 32px;
  cursor: pointer;
  align-items: center;
  gap: 8px;
  color: var(--mn-ink-muted);
  font-size: 13px;
  list-style: none;
}

.mn-control-details > summary::after {
  content: "+";
  font-size: 16px;
}

.mn-control-details[open] > summary::after {
  content: "−";
}

.mn-control-details > summary::-webkit-details-marker {
  display: none;
}

.mn-control-details[open] > :not(summary) {
  margin-top: 7px;
}

@media (max-width: 420px) {
  .mn-control-metric { padding: 10px; }
  .mn-control-service-row { align-items: flex-start; }
  .mn-control-service-value { gap: 6px; }
  .mn-control-service-value .mn-control-muted { flex-basis: calc(100% - 16px); margin-left: 15px; }
}

.mn-wifi-mode-btn,
:deep(.mn-wifi-mode-btn) {
  min-height: 40px;
  align-items: center;
  border-radius: var(--mn-radius-md);
  border-color: var(--mn-border-strong);
  color: var(--mn-ink);
  text-align: left;
  transition: border-color 150ms ease-out, background-color 150ms ease-out;
}

.mn-wifi-mode-btn-active,
:deep(.mn-wifi-mode-btn-active) {
  border-color: var(--mn-primary);
  background: color-mix(in srgb, var(--mn-primary) 9%, transparent);
  color: var(--mn-primary-strong);
}

.mn-wifi-mode-btn-inactive,
:deep(.mn-wifi-mode-btn-inactive) {
  background: transparent;
  color: var(--mn-ink-muted);
}

.mn-wifi-mode-btn:hover,
:deep(.mn-wifi-mode-btn:hover) {
  border-color: var(--mn-border);
  background: var(--mn-surface-sunken);
}

.mn-wifi-mode-btn:disabled,
:deep(.mn-wifi-mode-btn:disabled) {
  cursor: default;
}

.mn-wifi-switch {
  min-height: 36px;
  flex: 0 0 auto;
}

.mn-wifi-table th,
.mn-wifi-table td {
  padding: 9px 10px;
  border-bottom: 1px solid var(--mn-border);
  text-align: left;
}

.mn-wifi-table th {
  color: var(--mn-ink-muted);
  font-size: 12px;
  font-weight: 600;
}

.mn-hotspot-switch {
  position: relative;
  display: flex;
  min-height: 64px;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.mn-hotspot-row {
  display: flex;
  align-items: center;
  gap: 2px;
}

.mn-hotspot-row .mn-hotspot-switch {
  flex: 1 1 auto;
}

.mn-help-popover {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
  border: 1px solid var(--mn-border);
  border-radius: var(--mn-radius-md);
  padding: 9px 10px;
  background: var(--mn-surface-sunken);
  color: var(--mn-ink-muted);
  font-size: 12px;
  line-height: 1.6;
}

.mn-help-popover p { margin: 0; }

.mn-wifi-rules-details > summary {
  color: var(--mn-ink);
  font-size: 14px;
  font-weight: 550;
}

.mn-hotspot-input {
  position: absolute;
  inset: 0;
  z-index: 1;
  width: 100%;
  height: 100%;
  margin: 0;
  cursor: pointer;
  opacity: 0;
}

.mn-hotspot-input:disabled {
  cursor: not-allowed;
}

.mn-hotspot-label {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 16px;
  font-weight: 500;
}

.mn-hotspot-help {
  flex: 0 0 auto;
  width: 28px;
  height: 28px;
  min-width: 28px;
  min-height: 28px;
  padding: 0;
  border-radius: 6px;
}

.mn-hotspot-state {
  display: block;
  margin-top: 5px;
  color: var(--mn-ink-muted);
  font-size: 13px;
}

.mn-hotspot-track {
  position: relative;
  width: 50px;
  height: 30px;
  flex: 0 0 50px;
  border: 1px solid var(--mn-border-strong);
  border-radius: 99px;
  background: var(--mn-carrier-deep);
}

.mn-hotspot-track::after {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 22px;
  height: 22px;
  border-radius: 50%;
  background: var(--mn-surface-raised);
  content: "";
  transition: transform 150ms ease-out;
}

.mn-hotspot-input:checked ~ .mn-hotspot-track {
  border-color: var(--mn-primary);
  background: var(--mn-primary);
}

.mn-hotspot-input:checked ~ .mn-hotspot-track::after {
  background: var(--mn-on-accent);
  transform: translateX(20px);
}

.mn-hotspot-input:focus-visible ~ .mn-hotspot-track {
  outline: 2px solid var(--mn-focus);
  outline-offset: 4px;
}

.mn-hotspot-input:disabled ~ .mn-hotspot-track {
  opacity: 0.45;
}

.mn-control-confirm :deep(.magic-card) {
  border: 0;
  padding: 0;
  background: transparent;
}
</style>
