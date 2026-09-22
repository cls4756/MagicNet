<script setup lang="ts">
import { FileText, Globe2, Plus, Trash2 } from "lucide-vue-next";
import { computed } from "vue";
import { t } from "@/i18n";
import Button from "@/components/ui/Button.vue";
import type { SubscriptionUsageRow } from "@/composables/subscriptionUsage";

const props = withDefaults(defineProps<{
  lines: string[];
  usageRows: SubscriptionUsageRow[];
  editable?: boolean;
  local?: boolean;
  disabled?: boolean;
  max?: number;
}>(), {
  editable: false,
  local: false,
  disabled: false,
  max: 5,
});

const emit = defineEmits<{
  update: [index: number, value: string];
  add: [];
  remove: [index: number];
}>();

const canAdd = computed(() => props.editable && props.lines.length < props.max && !props.disabled);

function updateLine(index: number, event: Event): void {
  const target = event.target;
  if (!(target instanceof HTMLInputElement)) return;
  emit("update", index, target.value);
}

function sourceLabel(value: string): string {
  try {
    const url = new URL(value);
    return `${url.hostname}${url.pathname === "/" ? "" : url.pathname}`;
  } catch {
    return value;
  }
}
</script>

<template>
  <section class="subscription-source-list" :aria-label="t('订阅来源列表')">
    <header class="source-list-heading">
      <div class="source-list-title">
        <FileText :size="19" aria-hidden="true" />
        <div>
          <h3>{{ local ? t("本地订阅文件") : t("订阅来源") }}</h3>
          <p v-if="local">{{ t("当前使用本地文件；如需切换 URL 订阅，请导入新的订阅来源。") }}</p>
          <p v-else>{{ t("每一行是一个独立订阅源；相同节点可能同时出现在多个代理组中。") }}</p>
        </div>
      </div>
      <Button v-if="!local && editable" variant="outline" size="sm" :disabled="!canAdd" @click="emit('add')">
        <Plus :size="15" />{{ t("添加一行") }}
      </Button>
    </header>

    <div v-if="local" class="local-source-row">
      <FileText :size="18" aria-hidden="true" />
      <div>
        <strong>{{ t("本地订阅文件") }}</strong>
        <span>{{ t("节点由该文件解析，更新订阅不会读取 URL 用量。") }}</span>
      </div>
    </div>

    <div v-else-if="lines.length" class="source-card-grid">
      <article v-for="(line, index) in lines" :key="`source-${index}`" class="source-card" :data-active="index === 0 && !editable">
        <header class="source-card-header">
          <div class="source-card-identity">
            <span class="source-index" aria-hidden="true">{{ index + 1 }}</span>
            <div>
              <h4>{{ usageRows[index]?.hostname || sourceLabel(line) || t("订阅 {value}", { value: index + 1 }) }}</h4>
              <span>{{ t("订阅 {value}", { value: index + 1 }) }}<span v-if="usageRows[index]"> · {{ usageRows[index].stateLabel }}</span></span>
            </div>
          </div>
          <Button
            v-if="editable"
            variant="ghost"
            size="icon"
            :disabled="disabled || lines.length <= 1"
            :aria-label="t('删除订阅 {value}', { value: index + 1 })"
            :title="lines.length <= 1 ? t('至少保留一个订阅来源') : t('删除订阅 {value}', { value: index + 1 })"
            @click="emit('remove', index)"
          >
            <Trash2 :size="16" aria-hidden="true" />
          </Button>
        </header>
        <input
          v-if="editable"
          class="source-card-input"
          type="url"
          :value="line"
          :placeholder="t('HTTPS 订阅 URL')"
          :aria-label="t('订阅 {value} URL', { value: index + 1 })"
          autocomplete="off"
          autocapitalize="none"
          autocorrect="off"
          spellcheck="false"
          @input="updateLine(index, $event)"
        >
        <template v-else>
          <div v-if="usageRows[index]?.progressPercent !== null" class="source-card-progress">
            <progress max="100" :value="usageRows[index].progressPercent" :aria-label="t('订阅 {value} 已用流量', { value: index + 1 })" />
          </div>
          <p class="source-card-usage" v-if="usageRows[index]">
            {{ usageRows[index].usedLabel }} / {{ usageRows[index].totalLabel }}<span v-if="usageRows[index].expiryLabel"> · {{ usageRows[index].expiryLabel }}</span>
          </p>
          <p v-else class="source-card-usage">{{ t("尚未获取") }}</p>
        </template>
      </article>
    </div>

    <div v-else class="source-list-empty">
      <Globe2 :size="18" aria-hidden="true" />
      <span>{{ t("还没有订阅来源，点击“添加一行”开始。") }}</span>
    </div>

    <p v-if="!local" class="source-list-footnote">
      {{ t("最多保存 {value} 个订阅来源；完整地址只在编辑时显示，保存后由 MagicNet 拉取并解析节点。", { value: max }) }}
    </p>
  </section>
</template>

<style scoped>
.subscription-source-list { display: grid; gap: 14px; min-width: 0; }
.source-list-heading, .source-list-title, .local-source-row { display: flex; align-items: center; min-width: 0; }
.source-list-heading { justify-content: space-between; gap: 12px; }
.source-list-title { gap: 12px; }
.source-list-title > svg, .local-source-row > svg, .source-list-empty > svg { flex: 0 0 auto; color: var(--mn-ink-muted); }
.source-list-title h3 { margin: 0; color: var(--mn-ink); font-size: .9375rem; font-weight: 600; }
.source-list-title p { margin: 4px 0 0; color: var(--mn-ink-muted); font-size: .8125rem; line-height: 1.55; }
.source-card-grid { display: grid; gap: 12px; }
.source-card { display: grid; gap: 14px; min-width: 0; border: 1px solid var(--mn-border); border-radius: 18px; padding: 18px; background: var(--mn-surface-sunken); }
.source-card[data-active="true"] { border-color: var(--mn-primary); background: color-mix(in srgb, var(--mn-primary) 12%, var(--mn-surface-sunken)); }
.source-card-header { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; min-width: 0; }
.source-card-identity { display: flex; align-items: center; gap: 12px; min-width: 0; }
.source-card-identity h4 { margin: 0; color: var(--mn-ink); font-size: 1.05rem; font-weight: 650; overflow-wrap: anywhere; }
.source-card-identity > div > span { display: block; margin-top: 4px; color: var(--mn-ink-muted); font-size: .75rem; }
.source-index { display: grid; place-items: center; flex: 0 0 auto; width: 28px; height: 28px; border: 1px solid var(--mn-border-strong); border-radius: 50%; color: var(--mn-ink-muted); font-size: .75rem; font-variant-numeric: tabular-nums; }
.source-card-input { width: 100%; min-width: 0; border: 1px solid var(--mn-border-strong); border-radius: 10px; padding: 12px; color: var(--mn-ink); background: var(--mn-ivory); font: inherit; font-size: .875rem; outline: none; }
.source-card-input:focus { border-color: var(--mn-primary); box-shadow: 0 0 0 2px color-mix(in srgb, var(--mn-primary) 20%, transparent); }
.source-card-progress progress { display: block; appearance: none; width: 100%; height: 5px; border: 0; border-radius: 999px; overflow: hidden; background: var(--mn-carrier); }
.source-card-progress progress::-webkit-progress-bar { background: var(--mn-carrier); }
.source-card-progress progress::-webkit-progress-value { background: var(--mn-primary); border-radius: 999px; }
.source-card-usage { margin: 0; color: var(--mn-ink-soft); font-size: .875rem; line-height: 1.5; }
.local-source-row { gap: 12px; border: 1px solid var(--mn-border); border-radius: var(--mn-radius-sm); padding: 14px; background: var(--mn-surface-sunken); }
.local-source-row div { display: grid; gap: 3px; min-width: 0; }
.local-source-row strong { color: var(--mn-ink-soft); font-size: .875rem; }
.local-source-row span, .source-list-footnote, .source-list-empty { color: var(--mn-ink-muted); font-size: .8125rem; line-height: 1.6; }
.source-list-empty { display: flex; align-items: center; gap: 10px; border: 1px dashed var(--mn-border-strong); padding: 16px; }
.source-list-footnote { margin: 0; }
@media (max-width: 480px) {
  .source-list-heading { align-items: flex-start; flex-direction: column; }
  .source-list-heading > :last-child { width: 100%; }
  .source-list-heading > :last-child :deep(.mn-button) { width: 100%; }
}
</style>
