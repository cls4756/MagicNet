<script setup lang="ts">
import {
  Copy,
  Save,
  ShieldCheck,
  Unplug,
  Zap,
} from "lucide-vue-next";
import { computed, nextTick, onDeactivated, ref, watch } from "vue";
import { t } from "@/i18n";
import Button from "@/components/ui/Button.vue";
import Card from "@/components/ui/Card.vue";
import GroupSection from "@/components/ui/GroupSection.vue";
import ConfirmPanel from "@/components/ui/ConfirmPanel.vue";
import {
  applyConfigAction,
  type ControlDangerAction,
  repairAction,
  stopAllServicesAction,
} from "@/components/pages/controlDangerActions";
import { useActionLock } from "@/composables/useActionLock";
import { useMagicNet } from "@/composables/useMagicNet";
import { restoreFocusAfterUpdate, trapFocusWithin } from "@/lib/focus";
import { controlRuntimeBusy } from "@/components/pages/controlRuntimeInsight";
import { copyText } from "@/utils";

const {
  state,
  runCli,
  refreshAll,
  startBackgroundCli,
  refreshStatus,
} = useMagicNet();
const { isRunning, withAction } = useActionLock();

const pendingDangerAction = ref<ControlDangerAction | null>(null);
const dangerConfirmCard = ref<HTMLElement | null>(null);
const snapshotCopied = ref(false);
let dangerActionTrigger: HTMLElement | null = null;

const pendingDangerMessage = computed(
  () => pendingDangerAction.value?.message ?? "",
);

const runtimeBusy = computed(() =>
  controlRuntimeBusy(state.phase, state.queueDepth, state.backgroundTask.status),
);

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
  await withAction(action.key, async () => {
    if (action.background) {
      await startBackgroundCli(action.args, action.label);
      window.setTimeout(() => void refreshStatus(), 1200);
    } else {
      await runCli(action.args, action.label);
      await refreshAll();
    }
  });
}

function classifyLastCommand(command: string): string {
  if (!command) return "none";
  if (/\bbackup\b/.test(command)) return "backup";
  if (/\bsub(?:scription)?\b|sub set-file|subscription/i.test(command))
    return "subscription";
  if (/\btransparent\b/.test(command)) return "transparent";
  if (/\bservice\b/.test(command)) return "service";
  if (/\bconfig\b/.test(command)) return "config";
  if (/\bmcp\b/.test(command)) return "mcp";
  if (/\bwebui\b/.test(command)) return "webui";
  return "other";
}

function sanitizeControlSnapshot(text: string): string {
  return text
    .replace(/https?:\/\/\S+/gi, "[filtered-url]")
    .replace(
      /\b(token|secret|password|passwd|authorization|bearer|api[_-]?key|key)\b\s*[:=]\s*\S+/gi,
      "$1=[filtered]",
    );
}

async function copyControlSnapshot(): Promise<void> {
  const report = [
    "MagicNet control snapshot",
    `has_ksu=${state.hasKsu ? 1 : 0}`,
    `phase=${state.phase}`,
    `task=${state.task || "none"}`,
    `queue_depth=${state.queueDepth}`,
    `sing_box_state=${state.runtime.singBoxState}`,
    `sing_box=${state.runtime.singBox}`,
    `fswatch=${state.runtime.fswatch}`,
    `transparent_mode=${state.runtime.transparentMode}`,
    `transparent_effective_mode=${state.runtime.transparentEffectiveMode}`,
    `transparent_capability=${state.runtime.transparentCapability}`,
    `transparent_local_cgroup=${state.runtime.transparentLocalCgroup}`,
    `transparent_shared_tc=${state.runtime.transparentSharedTc}`,
    `transparent_shared_interfaces=${state.runtime.transparentSharedInterfaces.join(", ") || (state.runtime.transparentSharedInterfaceCount == null ? "unknown" : String(state.runtime.transparentSharedInterfaceCount))}`,
    `transparent_transition=${state.runtime.transparentTransition}`,
    `last_command_kind=${classifyLastCommand(state.lastCommand)}`,
  ].join("\n");
  snapshotCopied.value = await copyText(sanitizeControlSnapshot(report));
  state.output = snapshotCopied.value
    ? t("控制状态快照已复制。")
    : t("剪贴板不可用，控制状态快照未复制。");
}
</script>

<template>
  <GroupSection>
    <div class="mn-settings-stack">
      <Card class="grid gap-4">
        <div class="flex items-center justify-between gap-3">
          <h3 class="mn-card-title text-lg font-medium text-[var(--mn-ink)]">{{ t("服务管理") }}</h3>
        </div>

        <div class="grid grid-cols-2 gap-3">
          <Button variant="secondary" :disabled="runtimeBusy || !state.hasKsu" :loading="isRunning('apply-config')" @click="requestDangerAction(applyConfigAction(), $event.currentTarget)">
            <Save :size="17" />{{ t("应用配置") }} </Button>
          <Button variant="secondary" :disabled="runtimeBusy || !state.hasKsu" :loading="isRunning('repair')" @click="requestDangerAction(repairAction(), $event.currentTarget)">
            <Zap :size="17" />{{ t("自修复") }} </Button>
          <Button variant="outline" :disabled="!state.hasKsu" :loading="isRunning('api-groups')" @click="withAction('api-groups', () => runCli('api groups', t('检查 sing-box API')))">
            <ShieldCheck :size="17" />{{ t("检查 API") }} </Button>
          <Button variant="outline" @click="copyControlSnapshot"><Copy :size="17" />{{ snapshotCopied ? t("已复制") : t("复制快照") }}</Button>
          <Button variant="outline" :disabled="runtimeBusy || !state.hasKsu" :loading="isRunning('stop-all')" @click="requestDangerAction(stopAllServicesAction(), $event.currentTarget)">
            <Unplug :size="17" />{{ t("停止全部") }} </Button>
        </div>
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
  </GroupSection>
</template>
