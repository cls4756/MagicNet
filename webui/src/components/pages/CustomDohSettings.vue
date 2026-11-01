<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { t } from "@/i18n";
import Button from "@/components/ui/Button.vue";
import Input from "@/components/ui/Input.vue";
import ConfirmPanel from "@/components/ui/ConfirmPanel.vue";
import { useActionLock } from "@/composables/useActionLock";
import { useMagicNet } from "@/composables/useMagicNet";
import { buildCustomDohPayload, parseCustomDohSnapshot, supportsCustomDoh, type CustomDohSnapshot } from "./customDohPlan";

const props = defineProps<{ enabled: boolean }>();
const emit = defineEmits<{ saved: []; cancel: [] }>();
const { runCli, runPrivateCli, stagePrivatePayload, removePrivatePayload, shellQuote } = useMagicNet();
const { isRunning, withAction } = useActionLock();
const snapshot = ref<CustomDohSnapshot | null>(null);
const url = ref("");
const viaProxy = ref(true);
const pendingPayload = ref("");
const error = ref("");
const status = ref("");
const busy = computed(() => isRunning("custom-doh-load") || isRunning("custom-doh-save"));
let disposed = false;
let readSequence = 0;

async function load(): Promise<void> {
  if (busy.value) return;
  await withAction("custom-doh-load", async () => {
    const sequence = ++readSequence;
    error.value = "";
    const capabilities = await runCli("--json capabilities", t("检查自定义 DoH 支持"));
    if (disposed || sequence !== readSequence) return;
    if (!supportsCustomDoh(capabilities)) {
      snapshot.value = null;
      error.value = t("当前模块不支持自定义 DoH，请升级模块后重试。");
      return;
    }
    const outcome = await runPrivateCli("--json dns custom inspect", t("读取自定义 DoH 配置"), "--json dns custom inspect");
    if (disposed || sequence !== readSequence) return;
    const next = outcome.ok ? parseCustomDohSnapshot(outcome.stdout) : null;
    if (!next) {
      snapshot.value = null;
      error.value = t("自定义 DoH 配置读取失败，请重试或检查健康状态。");
      return;
    }
    snapshot.value = next;
    url.value = next.url;
    viaProxy.value = next.viaProxy;
    pendingPayload.value = "";
  });
}

function requestSave(): void {
  if (!props.enabled || !snapshot.value || busy.value) return;
  try {
    pendingPayload.value = JSON.stringify(buildCustomDohPayload(snapshot.value, url.value, viaProxy.value));
    error.value = "";
    status.value = "";
  } catch {
    error.value = t("请输入有效的 HTTPS DoH 地址，不支持用户名、密码或片段。");
  }
}

async function save(): Promise<void> {
  if (!props.enabled || !pendingPayload.value || busy.value) return;
  const payload = pendingPayload.value;
  let saved = false;
  await withAction("custom-doh-save", async () => {
    const basename = `custom-doh-${Date.now()}-${Math.random().toString(36).slice(2, 8)}.json`;
    const staged = await stagePrivatePayload("tmp", basename, payload, t("自定义 DoH 私有载荷"));
    if (!staged) {
      if (!disposed) error.value = t("安全临时数据写入失败，自定义 DoH 未保存。");
      return;
    }
    try {
      if (disposed || !props.enabled) return;
      const outcome = await runPrivateCli(`dns custom save-file ${shellQuote(staged.path)}`, t("校验并应用自定义 DoH"), "dns custom save-file [private-payload]");
      if (disposed) return;
      pendingPayload.value = "";
      if (!outcome.ok) {
        error.value = t("自定义 DoH 校验或应用失败。修改已保留，请重新读取配置或检查健康状态。");
        return;
      }
      status.value = t("自定义 DoH 已校验、保存并应用。");
      saved = true;
      emit("saved");
    } finally {
      const cleaned = await removePrivatePayload("tmp", staged.basename, t("自定义 DoH 私有载荷"));
      if (!cleaned && !disposed) error.value = t("自定义 DoH 私有临时数据清理未确认。");
    }
  });
  if (saved && !disposed) await load();
}

function cancel(): void {
  if (busy.value) return;
  pendingPayload.value = "";
  if (snapshot.value) { url.value = snapshot.value.url; viaProxy.value = snapshot.value.viaProxy; }
  error.value = "";
  status.value = "";
  emit("cancel");
}

onMounted(() => { void load(); });
onBeforeUnmount(() => { disposed = true; readSequence += 1; });
</script>

<template>
  <section class="custom-doh-settings grid gap-3 rounded-md border border-[color-mix(in_srgb,var(--mn-ink)_10%,transparent)] p-3" :aria-label="t('自定义 DoH')">
    <div>
      <h3 class="text-sm font-semibold">{{ t("自定义 DoH") }}</h3>
      <p class="mt-1 text-xs leading-5 text-[var(--mn-ink-muted)]">{{ t("DoH 域名通过当前 Bootstrap DNS 引导解析；应用 DNS 规则决定哪些查询使用此地址。TLS 证书始终校验。") }}</p>
    </div>
    <label class="grid gap-1 text-xs text-[var(--mn-ink-muted)]">
      <span>{{ t("DoH 地址") }}</span>
      <Input v-model="url" type="url" placeholder="https://dns.example/dns-query" autocomplete="off" autocapitalize="none" spellcheck="false" :disabled="!enabled || !snapshot || busy || Boolean(pendingPayload)" />
    </label>
    <label class="grid gap-1 text-xs text-[var(--mn-ink-muted)]">
      <span>{{ t("DoH 连接方式") }}</span>
      <select v-model="viaProxy" class="mn-field min-h-12 rounded-md px-3 text-sm" :disabled="!enabled || !snapshot || busy || Boolean(pendingPayload)">
        <option :value="true">{{ t("通过代理") }}</option>
        <option :value="false">{{ t("直连") }}</option>
      </select>
    </label>
    <p class="text-xs leading-5 text-[var(--mn-ink-muted)]">{{ t("地址可能包含私密路径或参数，仅用于配置编辑，不写入状态、操作报告或浏览器存储。") }}</p>
    <p v-if="error" role="alert" class="text-xs text-[var(--mn-danger)]">{{ error }}</p>
    <p v-if="status" role="status" class="text-xs text-[var(--mn-success)]">{{ status }}</p>
    <ConfirmPanel v-if="pendingPayload" :title="t('校验并应用自定义 DoH')" :detail="t('保存后切换应用 DNS 到自定义 DoH，并重新应用运行配置；校验失败不保存，应用失败尝试恢复原配置。')" :loading="busy" @cancel="pendingPayload = ''" @confirm="save" />
    <div v-else class="flex flex-wrap gap-2">
      <Button size="sm" :disabled="!enabled || !snapshot || busy" @click="requestSave">{{ t("校验并应用自定义 DoH") }}</Button>
      <Button size="sm" variant="outline" :disabled="busy" @click="load">{{ t("重新读取配置") }}</Button>
      <Button size="sm" variant="ghost" :disabled="busy" @click="cancel">{{ t("取消") }}</Button>
    </div>
  </section>
</template>
