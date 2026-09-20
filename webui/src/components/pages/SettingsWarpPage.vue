<script setup lang="ts">
import { t } from "@/i18n";
import { Copy, Power, PowerOff, RadioTower, RefreshCw, Upload } from "lucide-vue-next";
import { computed, ref, watch } from "vue";
import Button from "@/components/ui/Button.vue";
import Card from "@/components/ui/Card.vue";
import PageHeader from "@/components/ui/PageHeader.vue";
import Textarea from "@/components/ui/Textarea.vue";
import { useActionLock } from "@/composables/useActionLock";
import { useMagicNet } from "@/composables/useMagicNet";
import { copyText, execFailed, redactedCliPreview } from "@/utils";
import ToolActionConfirmCard from "./ToolActionConfirmCard.vue";
import WarpRouteRulesPanel from "./WarpRouteRulesPanel.vue";
import type { PendingToolAction } from "./toolActions";
import { formatWarpImportSummaryReport, summarizeWarpImport, warpImportTone } from "./warpImportSummary";

const {
  state,
  runCli,
  runPrivateCli,
  stagePrivatePayload,
  removePrivatePayload,
  refreshWarp,
  shellQuote,
} = useMagicNet();
const { isRunning, withAction } = useActionLock();
const pendingWarpAction = ref<PendingToolAction | null>(null);
const warpSummaryCopied = ref(false);
const warpImportSummary = computed(() => summarizeWarpImport(state.warp.importText));
watch(() => state.warp.importText, () => { warpSummaryCopied.value = false; });

function requestWarpAction(action: PendingToolAction): void {
  pendingWarpAction.value = action;
}
function cancelWarpAction(): void {
  pendingWarpAction.value = null;
}
async function confirmWarpAction(): Promise<void> {
  const action = pendingWarpAction.value;
  if (!action) return;
  try {
    await action.run();
  } finally {
    pendingWarpAction.value = null;
  }
}

async function refreshWarpStatus(): Promise<void> {
  pendingWarpAction.value = null;
  await refreshWarp(true);
}

function warpPayloadBasename(): string {
  return `webui-warp-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}.conf`;
}

async function runImportWarp(payload: string): Promise<void> {
  await withAction("warp-import", async () => {
    const staged = await stagePrivatePayload(
      "tmp",
      warpPayloadBasename(),
      payload,
      t("WARP 导入载荷"),
    );
    if (!staged) {
      state.output = t("安全临时数据写入失败，WARP 导入未开始。");
      return;
    }
    try {
      const outcome = await runPrivateCli(
        "warp import-file " + shellQuote(staged.path),
        t("导入并启用 WARP"),
        redactedCliPreview("warp import-file [private-payload]"),
      );
      if (!outcome.ok) {
        state.output = t("WARP 导入失败，请检查配置后重试。");
        return;
      }
      state.warp.importText = "";
      state.output = t("WARP 配置已导入并应用。");
      if (!(await refreshWarp(true))) {
        state.output = t("WARP 配置已导入，但状态刷新未确认，请检查输出后重试。");
        return;
      }
      state.output = t("WARP 配置已导入并应用。");
    } finally {
      const cleaned = await removePrivatePayload("tmp", staged.basename, t("WARP 导入载荷"));
      if (!cleaned) {
        state.phase = "error";
        state.output = state.output + t("\n\nWARP 私有临时数据清理未确认。");
      }
    }
  });
}

function importWarp(): void {
  const payload = state.warp.importText.trim();
  if (!payload) {
    state.output = t("请先粘贴 WARP/WireGuard 配置。");
    return;
  }
  if (!warpImportSummary.value.looksImportable) {
    state.output = warpImportSummary.value.message;
    return;
  }
  requestWarpAction({
    key: "warp-import",
    get title() { return t("导入并启用 WARP"); },
    get detail() {
      const summary = summarizeWarpImport(payload);
      return t("会写入 WireGuard/WARP 配置，并让 MagicNet 应用新的 WARP 出站。{warning}", { warning: summary.status === "warning" ? ` ${summary.message}` : "" });
    },
    command: "warp import-file <config-file>",
    run: () => runImportWarp(payload),
  });
}

async function copyWarpSummary(): Promise<void> {
  warpSummaryCopied.value = await copyText(formatWarpImportSummaryReport(warpImportSummary.value));
  state.output = warpSummaryCopied.value ? t("WARP 导入摘要已复制。") : t("剪贴板不可用，WARP 导入摘要未复制。");
}

async function runSetWarpEnabled(enabled: boolean): Promise<void> {
  await withAction(enabled ? "warp-enable" : "warp-disable", async () => {
    const text = await runCli(enabled ? "warp enable" : "warp disable", enabled ? t("启用 WARP") : t("禁用 WARP"));
    if (execFailed(text)) return;
    state.output = text;
    if (!(await refreshWarp(true))) {
      state.output = t("WARP 开关已执行，但状态刷新未确认，请检查输出后重试。");
      return;
    }
    state.output = text || (enabled ? t("WARP 已启用。") : t("WARP 已禁用。"));
  });
}

function setWarpEnabled(enabled: boolean): void {
  requestWarpAction({
    key: enabled ? "warp-enable" : "warp-disable",
    get title() { return enabled ? t("启用 WARP") : t("禁用 WARP"); },
    get detail() { return enabled ? t("会启用当前 WARP 出站配置。") : t("会关闭 WARP 出站，已依赖 WARP 的路由会失效。"); },
    command: enabled ? "warp enable" : "warp disable",
    run: () => runSetWarpEnabled(enabled),
  });
}

async function testWarp(): Promise<void> {
  await withAction("warp-test", async () => {
    state.output = await runCli("warp test", t("测试 WARP"));
  });
}

async function runSelectWarpGlobal(enabled: boolean): Promise<void> {
  await withAction(enabled ? "warp-global" : "warp-rule", async () => {
    state.output = await runCli(enabled ? "warp global" : "warp rule", enabled ? t("全局走 WARP") : t("恢复规则出站"));
  });
}

function selectWarpGlobal(enabled: boolean): void {
  requestWarpAction({
    key: enabled ? "warp-global" : "warp-rule",
    get title() { return enabled ? t("全局走 WARP") : t("恢复规则出站"); },
    get detail() { return enabled ? t("会把默认出站切到 WARP。") : t("会恢复按规则选择出站。"); },
    command: enabled ? "warp global" : "warp rule",
    run: () => runSelectWarpGlobal(enabled),
  });
}
</script>

<template>
  <div class="grid gap-4">
    <PageHeader :overline="t('设置')" :title="t('WARP 出站')" />

    <ToolActionConfirmCard
      v-if="pendingWarpAction"
      :action="pendingWarpAction"
      :loading="isRunning(pendingWarpAction.key)"
      @cancel="cancelWarpAction"
      @confirm="confirmWarpAction"
    />

    <Card class="grid gap-3">
      <p class="text-sm leading-6 text-[var(--mn-ink-muted)]">{{ t("导入 WireGuard 配置后，可全局使用 WARP，或仅用于指定域名。") }}</p>
      <Textarea v-model="state.warp.importText" class="min-h-32 font-mono text-xs" spellcheck="false" placeholder="[Interface]&#10;PrivateKey = ...&#10;Address = ...&#10;&#10;[Peer]&#10;PublicKey = ...&#10;Endpoint = ...:2408" />
      <div class="rounded-md border p-3 text-sm leading-6" :class="warpImportTone(warpImportSummary.status)">
        <p class="font-medium">{{ warpImportSummary.status === 'ok' ? t("可导入") : warpImportSummary.status === 'warning' ? t("可导入但需确认") : warpImportSummary.status === 'error' ? t("不能导入") : t("等待配置") }}</p>
        <p class="mt-1 text-xs opacity-80">{{ warpImportSummary.message }}</p>
        <p class="mt-2 text-xs opacity-80">
          Interface {{ warpImportSummary.hasInterface ? t("存在") : t("缺失") }} · Peer {{ warpImportSummary.hasPeer ? t("存在") : t("缺失") }} · Address {{ warpImportSummary.hasAddress ? t("存在") : t("缺失") }} · Endpoint {{ warpImportSummary.hasEndpoint ? t("存在") : t("缺失") }}
        </p>
      </div>
      <div class="grid gap-2 sm:grid-cols-2">
        <Button :disabled="!warpImportSummary.looksImportable" :loading="isRunning('warp-import')" @click="importWarp"><Upload :size="16" />{{ t("导入并启用") }}</Button>
        <Button variant="outline" :disabled="!state.warp.importText.trim()" @click="copyWarpSummary"><Copy :size="16" />{{ warpSummaryCopied ? t("已复制摘要") : t("复制导入摘要") }}</Button>
        <Button variant="secondary" :disabled="!state.warp.configured" :loading="isRunning('warp-test')" @click="testWarp"><RadioTower :size="16" />{{ t("测试 WARP") }}</Button>
        <Button variant="outline" :disabled="!state.warp.configured || state.warp.enabled" :loading="isRunning('warp-enable')" @click="setWarpEnabled(true)"><Power :size="16" />{{ t("启用") }}</Button>
        <Button variant="outline" :disabled="!state.warp.enabled" :loading="isRunning('warp-disable')" @click="setWarpEnabled(false)"><PowerOff :size="16" />{{ t("禁用") }}</Button>
        <Button variant="secondary" :disabled="!state.warp.enabled" :loading="isRunning('warp-global')" @click="selectWarpGlobal(true)">{{ t("全局走 WARP") }}</Button>
        <Button variant="outline" :loading="isRunning('warp-rule')" @click="selectWarpGlobal(false)">{{ t("恢复规则") }}</Button>
        <Button variant="outline" :loading="isRunning('warp-refresh')" @click="withAction('warp-refresh', refreshWarpStatus)"><RefreshCw :size="16" />{{ t("刷新") }}</Button>
      </div>
      <WarpRouteRulesPanel />
      <pre class="max-h-44 overflow-auto rounded-md bg-[var(--mn-carrier-deep)] p-3 text-xs leading-6 text-[var(--mn-ink-soft)] whitespace-pre-wrap">enabled={{ state.warp.enabled ? "1" : "0" }}
configured={{ state.warp.configured ? "1" : "0" }}
tag={{ state.warp.tag }}
endpoint={{ state.warp.endpoint || "-" }}
addresses={{ state.warp.addresses }}
allowed_ips={{ state.warp.allowedIps }}</pre>
    </Card>
  </div>
</template>