<script setup lang="ts">
import { computed, ref } from "vue";
import { RefreshCw } from "lucide-vue-next";
import { t } from "@/i18n";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import Card from "@/components/ui/Card.vue";
import CardHeading from "@/components/ui/CardHeading.vue";
import { parseRoutingInspectSnapshot, type RoutingOutboundSummary } from "@/composables/routingInspect";
import { useActionLock } from "@/composables/useActionLock";
import { useMagicNet } from "@/composables/useMagicNet";
import { useVisibilityTask } from "@/composables/useVisibilityTask";
import { execFailed } from "@/utils";

const { runCli } = useMagicNet();
const { isRunning, withAction } = useActionLock();
const rawOutput = ref("");
const error = ref("");
const snapshot = computed(() => parseRoutingInspectSnapshot(rawOutput.value));
const rows = computed(() => (snapshot.value?.outbounds || []).filter((outbound) => outbound.internal));

function referenceSummary(outbound: RoutingOutboundSummary): string {
  if (!outbound.references.length) return t("当前未被规则或其他出站引用");
  return outbound.references.map((reference) => {
    if (reference.kind === "route_final") return t("路由兜底");
    if (reference.kind === "route_rule") return t("路由规则 {order}", { order: reference.order ?? "?" });
    return t("被策略组引用：{name}", { name: reference.tag || t("未知") });
  }).join("、");
}

async function refresh(): Promise<void> {
  await withAction("builtin-outbound-refresh", async () => {
    error.value = "";
    const output = await runCli("--json routing inspect", t("读取内置出站"), true);
    if (execFailed(output) || !parseRoutingInspectSnapshot(output)) {
      error.value = t("读取内置出站失败，请检查当前配置。");
      return;
    }
    rawOutput.value = output;
  });
}

const { target: visibilityTarget } = useVisibilityTask(refresh);
</script>

<template>
  <div ref="visibilityTarget" class="mn-deferred-region">
    <Card class="grid gap-3">
      <CardHeading :title="t('内置出站')">
        <Button size="sm" variant="outline" :loading="isRunning('builtin-outbound-refresh')" @click="refresh">
          <RefreshCw :size="15" />{{ t("刷新") }}
        </Button>
      </CardHeading>
      <p class="text-sm leading-5 text-[var(--mn-ink-muted)]">{{ t("这里列出 MagicNet 生成的内部 outbound、实际选择、候选成员及路由或组引用。目前仅支持查看；新增、删除和编辑出站定义尚未开放。") }}</p>
      <p v-if="snapshot?.runtimeState === 'unknown'" class="text-xs text-[var(--mn-warning)]">{{ t("运行选择未知；当前选择列不以配置默认值代替运行状态。") }}</p>
      <p v-if="error" class="text-sm text-[var(--mn-danger)]">{{ error }}</p>
      <div class="overflow-x-auto">
        <table class="mn-outbound-table w-full text-sm">
          <thead>
            <tr>
              <th>{{ t("名称") }}</th>
              <th>{{ t("类型") }}</th>
              <th>{{ t("候选出站") }}</th>
              <th>{{ t("配置默认") }}</th>
              <th>{{ t("运行选择") }}</th>
              <th>{{ t("引用数") }}</th>
              <th>{{ t("引用用途") }}</th>
              <th>{{ t("管理") }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="row in rows" :key="row.tag">
              <td class="font-mono">{{ row.tag }}</td>
              <td>{{ row.type }}</td>
              <td :title="row.members.join(', ')">{{ row.members.length }}</td>
              <td class="font-mono">{{ row.default || t("无") }}</td>
              <td class="font-mono">{{ row.selected || t("未知") }}</td>
              <td>{{ row.referenceCount }}</td>
              <td class="mn-outbound-usage">{{ referenceSummary(row) }}</td>
              <td><Badge>{{ t("只读") }}</Badge></td>
            </tr>
            <tr v-if="!rows.length"><td colspan="8" class="mn-empty text-center">{{ error || t("暂无内置出站") }}</td></tr>
          </tbody>
        </table>
      </div>
      <p v-if="snapshot?.truncated" class="text-xs text-[var(--mn-warning)]">{{ t("配置数量超过展示上限，表格内容已截断。") }}</p>
    </Card>
  </div>
</template>

<style scoped>
.mn-outbound-table th,
.mn-outbound-table td { padding: 8px 9px; border-bottom: 1px solid var(--mn-border); text-align: left; white-space: nowrap; }
.mn-outbound-table th { color: var(--mn-ink-muted); font-size: 12px; font-weight: 600; }
.mn-outbound-usage { min-width: 220px; max-width: 420px; white-space: normal !important; }
</style>
