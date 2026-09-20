<script setup lang="ts">
import { t } from "@/i18n";
import { ClipboardPaste, Copy, Download, FileLock, Network, RefreshCw, Upload } from "lucide-vue-next";
import { computed, ref, watch } from "vue";
import Button from "@/components/ui/Button.vue";
import Card from "@/components/ui/Card.vue";
import Input from "@/components/ui/Input.vue";
import PageHeader from "@/components/ui/PageHeader.vue";
import Textarea from "@/components/ui/Textarea.vue";
import { useActionLock } from "@/composables/useActionLock";
import { useMagicNet } from "@/composables/useMagicNet";
import { copyText, execFailed, readClipboardText, redactedCliPreview } from "@/utils";
import ToolActionConfirmCard from "./ToolActionConfirmCard.vue";
import EcaptureToolsCard from "./EcaptureToolsCard.vue";
import McpToolsCard from "./McpToolsCard.vue";
import NetworkSnapshotPanel from "./NetworkSnapshotPanel.vue";
import { summarizeBackupPayload } from "./backupPayloadSummary";
import type { PendingToolAction } from "./toolActions";
import { summarizeRefreshTools } from "./refreshToolsState";

const {
  state,
  runCli,
  runPrivateCli,
  stagePrivatePayload,
  removePrivatePayload,
  refreshMcp,
  refreshTopology,
  refreshSysroute,
  shellQuote,
} = useMagicNet();
const { isRunning, withAction } = useActionLock();
const toolsRefreshing = ref(false);
const pendingToolAction = ref<PendingToolAction | null>(null);
const backupSummaryCopied = ref(false);
const backupPayloadSummary = computed(() => summarizeBackupPayload(state.backup.payload.trim()));
watch(() => state.backup.payload, () => { backupSummaryCopied.value = false; });

async function refreshTools(): Promise<void> {
  toolsRefreshing.value = true;
  state.task = t("刷新工具状态");
  try {
    const steps = [
      { label: "MCP", ok: await refreshMcp(true) },
    ] as const;
    const summary = summarizeRefreshTools(steps, state.output);
    state.notice = summary.notice;
    state.output = summary.output;
    state.phase = summary.completed ? "done" : "error";
  } finally {
    toolsRefreshing.value = false;
    state.task = "";
  }
}
function requestToolAction(action: PendingToolAction): void {
  pendingToolAction.value = action;
}
function cancelToolAction(): void {
  pendingToolAction.value = null;
}
async function confirmToolAction(): Promise<void> {
  const action = pendingToolAction.value;
  if (!action) return;
  try {
    await action.run();
  } finally {
    pendingToolAction.value = null;
  }
}
async function exportBackup(): Promise<void> {
  await withAction("backup-export", async () => {
    const securityCode = state.backup.exportPassword.trim();
    const preview = redactedCliPreview("backup export [备份内容和安全码已隐藏]");
    state.output = t("正在安全导出备份...");
    const outcome = await runPrivateCli(
      securityCode ? `backup export ${shellQuote(securityCode)}` : "backup export",
      t("导出配置备份"),
      preview,
    );
    const payload = outcome.ok ? outcome.stdout.trim().split(/\s+/).pop() || "" : "";
    const exported = outcome.ok && Boolean(payload);
    state.phase = exported ? "done" : "error";
    if (!exported) {
      state.backup.status = t("导出失败，请检查设备状态后重试。");
      state.output = state.backup.status;
      return;
    }
    state.backup.payload = payload;
    state.backup.status = t("已导出到备份输入框；为保护敏感内容，未自动复制到剪切板。");
    state.output = state.backup.status;
  });
}

async function pasteBackup(): Promise<void> {
  await withAction("backup-paste", async () => {
    const text = (await readClipboardText()).trim();
    if (!text) {
      state.backup.status = t("剪切板为空或不可读取，请手动粘贴");
      state.output = state.backup.status;
      return;
    }
    state.backup.payload = text;
    state.backup.status = t("已从剪切板读取 {length} 字符", { length: text.length });
  });
}

async function copyBackupSummary(): Promise<void> {
  const summary = backupPayloadSummary.value;
  const report = [
    "MagicNet backup payload summary",
    "privacy_note=payload body is not included",
    `payload_present=${state.backup.payload.trim() ? 1 : 0}`,
    `looks_valid=${summary.looksValid ? 1 : 0}`,
    `lines=${summary.lines}`,
    `chars=${summary.chars}`,
    `compact_chars=${summary.compactChars}`,
    `has_whitespace=${summary.hasWhitespace ? 1 : 0}`,
    `invalid_chars=${summary.invalidChars ? 1 : 0}`,
    `too_large=${summary.tooLarge ? 1 : 0}`,
    `fnv32=${summary.fingerprint}`
  ].join("\n");
  backupSummaryCopied.value = await copyText(report);
  state.output = backupSummaryCopied.value ? t("备份摘要已复制。") : t("剪贴板不可用，备份摘要未复制。");
}

async function runRestoreBackup(payload: string): Promise<void> {
  await withAction("backup-restore", async () => {
    const rawCode = state.backup.restorePassword.trim();
    const securityCode = rawCode || "-";
    const staged = await stageBackupPayload(payload);
    if (!staged) {
      state.backup.status = t("安全临时数据写入失败，导入未开始。");
      state.output = state.backup.status;
      return;
    }
    try {
      const outcome = await runPrivateCli(
        `backup restore-file ${shellQuote(securityCode)} ${shellQuote(staged.path)}`,
        t("导入配置备份"),
        redactedCliPreview("backup restore-file [安全码和备份内容已隐藏]"),
      );
      const restored = outcome.ok && outcome.stdout.includes("[info] Backup restored");
      state.phase = restored ? "done" : "error";
      state.backup.status = restored
        ? t("导入成功，运行配置已应用")
        : t("导入失败，请检查安全码和备份内容后重试。");
      state.output = state.backup.status;
    } finally {
      const cleaned = await removePrivatePayload("tmp", staged.basename, t("备份导入载荷"));
      if (!cleaned) {
        state.backup.status = t("{status} 私有临时数据清理未确认。", { status: state.backup.status });
        state.phase = "error";
        state.output = state.backup.status;
      }
    }
  });
}

function restoreBackup(): void {
  const payload = state.backup.payload.trim();
  if (!payload) {
    state.backup.status = t("请先粘贴备份字符串");
    state.output = state.backup.status;
    return;
  }
  if (!backupPayloadSummary.value.looksValid) {
    state.backup.status = t("备份字符串看起来不完整或包含非法字符");
    state.output = state.backup.status;
    return;
  }
  requestToolAction({
    key: "backup-restore",
    get title() { return t("导入配置备份"); },
    get detail() { return t("会覆盖设备上的订阅、应用名单、黑名单和路由等运行配置。"); },
    command: "backup restore-file <security-code> <payload-file>",
    run: () => runRestoreBackup(payload),
  });
}

function privatePayloadBasename(prefix: string, extension: string): string {
  return prefix
    + "-"
    + Date.now().toString(36)
    + "-"
    + Math.random().toString(36).slice(2, 8)
    + "."
    + extension;
}

async function stageBackupPayload(payload: string) {
  state.backup.status = t("正在安全暂存备份数据...");
  state.output = t("正在安全暂存备份数据...");
  return stagePrivatePayload(
    "tmp",
    privatePayloadBasename("webui-backup-restore", "b64"),
    payload,
    t("备份导入载荷"),
  );
}

</script>

<template>
  <div class="grid gap-4">
    <PageHeader :overline="t('工具')" :title="t('维护')">
      <Button variant="outline" :loading="toolsRefreshing" @click="refreshTools">
        <RefreshCw :size="17" />{{ t("刷新") }}
      </Button>
    </PageHeader>

    <ToolActionConfirmCard
      v-if="pendingToolAction"
      :action="pendingToolAction"
      :loading="isRunning(pendingToolAction.key)"
      @cancel="cancelToolAction"
      @confirm="confirmToolAction"
    />

    <div class="grid min-w-0 gap-3 md:grid-cols-2">
      <Card class="grid gap-3">
        <h3 class="inline-flex items-center gap-2 text-base font-semibold"><FileLock :size="17" /> {{ t("配置迁移") }}</h3>
        <p class="text-sm leading-6 text-[var(--mn-ink-muted)]">{{ t("导出会打包订阅、应用名单、黑名单、路由规则等用户配置。安全码可留空；设置后导入时必须填写一致。") }}</p>
        <div class="grid gap-2 sm:grid-cols-2">
          <Input v-model="state.backup.exportPassword" type="password" autocomplete="new-password" :placeholder="t('导出安全码，可留空')" />
          <Button :loading="isRunning('backup-export')" @click="exportBackup"><Download :size="16" />{{ t("导出到备份框") }}</Button>
        </div>
        <Textarea v-model="state.backup.payload" class="min-h-28" spellcheck="false" :placeholder="t('备份字符串会出现在这里，也可以手动粘贴剪切板内容')" />
        <div class="grid gap-2 rounded-md border border-[color-mix(in_srgb,var(--mn-ink)_12%,transparent)] bg-[var(--mn-ivory)] p-3 text-xs text-[var(--mn-ink-muted)] sm:grid-cols-4">
          <span>{{ t("行数：{count}", { count: backupPayloadSummary.lines }) }}</span>
          <span>{{ t("字符数：{count}", { count: backupPayloadSummary.chars }) }}</span>
          <span>{{ t("非空白字符数：{count}", { count: backupPayloadSummary.compactChars }) }}</span>
          <span :class="backupPayloadSummary.looksValid ? 'text-[var(--mn-success)]' : 'text-[var(--mn-warning)]'">
            {{ backupPayloadSummary.looksValid ? `fnv32:${backupPayloadSummary.fingerprint}` : backupPayloadSummary.tooLarge ? t("超过 5MiB") : backupPayloadSummary.invalidChars ? t("含非法字符") : backupPayloadSummary.hasWhitespace ? t("含空白，将压缩检查") : t("等待有效备份") }}
          </span>
        </div>
        <div class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto_auto_auto]">
          <Input v-model="state.backup.restorePassword" type="password" autocomplete="current-password" :placeholder="t('导入安全码，可留空')" />
          <Button variant="secondary" :loading="isRunning('backup-paste')" @click="pasteBackup"><ClipboardPaste :size="16" />{{ t("读剪切板") }}</Button>
          <Button variant="outline" :disabled="!state.backup.payload.trim()" @click="copyBackupSummary"><Copy :size="16" />{{ backupSummaryCopied ? t("已复制摘要") : t("复制摘要") }}</Button>
          <Button :loading="isRunning('backup-restore')" @click="restoreBackup"><Upload :size="16" />{{ t("导入配置") }}</Button>
        </div>
        <p class="text-xs leading-5 text-[var(--mn-ink-muted)]">{{ t(state.backup.status) }}</p>
      </Card>

      <EcaptureToolsCard />

    </div>

    <div class="grid min-w-0 gap-3 md:grid-cols-2">
      <McpToolsCard />

      <Card>
        <h3 class="mb-2 inline-flex items-center gap-2 text-base font-semibold"><Network :size="17" /> {{ t("拓扑 / 路由") }}</h3>
        <p class="text-sm leading-6 text-[var(--mn-ink-muted)]">{{ t("只读网络与路由快照。") }}</p>
        <div class="grid gap-2">
          <Button variant="secondary" :loading="isRunning('refresh-topology')" @click="withAction('refresh-topology', () => refreshTopology())">{{ t("刷新拓扑/路由") }}</Button>
          <Button variant="secondary" :loading="isRunning('refresh-sysroute')" @click="withAction('refresh-sysroute', () => refreshSysroute())">{{ t("刷新路由") }}</Button>
        </div>
      </Card>
    </div>

    <NetworkSnapshotPanel :topology="state.topology" :sysroute="state.sysroute" />
  </div>
</template>
