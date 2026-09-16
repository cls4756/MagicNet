<script setup lang="ts">
import { t } from "@/i18n";
import { computed, onMounted, ref } from "vue";
import { RefreshCw, Split } from "lucide-vue-next";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import Card from "@/components/ui/Card.vue";
import { useActionLock } from "@/composables/useActionLock";
import { useMagicNet } from "@/composables/useMagicNet";
import { execFailed } from "@/utils";

const { runCli, refreshDomainForward } = useMagicNet();
const { isRunning, withAction } = useActionLock();
const configured = ref<"enabled" | "disabled">("enabled");
const coreSupport = ref<"available" | "unavailable">("unavailable");
const effective = ref<"enabled" | "disabled" | "pending" | "unsupported">("pending");

const statusTone = computed(() => {
  if (effective.value === "enabled") return "success" as const;
  if (effective.value === "unsupported") return "danger" as const;
  if (effective.value === "pending") return "warning" as const;
  return "neutral" as const;
});

const statusText = computed(() => {
  if (effective.value === "enabled") return t("已生效");
  if (effective.value === "unsupported") return t("内核不支持");
  if (effective.value === "pending") return t("等待生效");
  return t("已关闭");
});

const statusHint = computed(() => {
  if (effective.value === "enabled") {
    return t("出站改用嗅探到的域名拨号，代理节点可以按域名分流；按 IP 直连的失败会回退重试一次。UDP 仍走 IP，不受影响。");
  }
  if (effective.value === "unsupported") {
    return t("当前 sing-box 内核没有域名覆写能力，开关已保存但不会写入运行配置。需要先合入补丁并更新内核。");
  }
  if (effective.value === "pending") {
    return t("开关已保存，但运行配置尚未带上覆写规则。重新应用配置或重启内核后生效。");
  }
  return t("已关闭：出站只使用 IP，节点无法按域名分流，嗅探结果仅用于日志和规则匹配。");
});

const supportText = computed(() =>
  coreSupport.value === "available" ? t("内核可用") : t("内核不可用"));

async function refreshStatus(silent = false): Promise<void> {
  const status = await refreshDomainForward(silent);
  if (!status) return;
  configured.value = status.configured;
  coreSupport.value = status.core_support;
  effective.value = status.effective;
}

async function setEnabled(enabled: boolean): Promise<void> {
  await withAction("domain-forward", async () => {
    const output = await runCli(
      `domain-forward ${enabled ? "enable" : "disable"}`,
      t("应用域名转发"),
    );
    if (!execFailed(output)) await refreshStatus(true);
  });
}

onMounted(() => void refreshStatus(true));
</script>

<template>
  <Card class="grid gap-3">
    <div class="flex items-center justify-between gap-3">
      <h3 class="inline-flex items-center gap-2 text-base font-semibold">
        <Split :size="17" /> {{ t("域名转发") }}
      </h3>
      <div class="flex items-center gap-2">
        <Badge :tone="statusTone">{{ statusText }}</Badge>
        <Button size="sm" variant="outline" :loading="isRunning('domain-forward-refresh')" @click="withAction('domain-forward-refresh', () => refreshStatus())">
          <RefreshCw :size="15" />{{ t("刷新") }}
        </Button>
      </div>
    </div>
    <p class="text-sm leading-6 text-[var(--mn-ink-muted)]">
      {{ t("让代理节点收到本地嗅探出的域名，而不是只有 IP。默认开启，只影响 TCP。") }}
    </p>
    <p class="rounded-md bg-[var(--mn-ivory)] px-3 py-2 text-xs leading-5 text-[var(--mn-ink-muted)]">
      {{ statusHint }}
    </p>
    <div class="flex flex-wrap gap-2">
      <Button :loading="isRunning('domain-forward')" :disabled="configured === 'enabled'" @click="setEnabled(true)">
        {{ t("开启") }}
      </Button>
      <Button variant="outline" :loading="isRunning('domain-forward')" :disabled="configured === 'disabled'" @click="setEnabled(false)">
        {{ t("关闭") }}
      </Button>
    </div>
    <pre class="overflow-auto rounded-md bg-[var(--mn-carrier-deep)] p-3 text-xs leading-6 text-[var(--mn-ink-soft)]">configured={{ configured }}
core_support={{ coreSupport }}
effective={{ effective }}</pre>
    <p class="text-xs leading-5 text-[var(--mn-ink-muted)]">{{ supportText }}</p>
  </Card>
</template>