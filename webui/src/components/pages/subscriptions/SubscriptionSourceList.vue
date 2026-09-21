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

    <div v-else-if="lines.length" class="source-list-rows">
      <article v-for="(line, index) in lines" :key="`source-${index}`" class="source-list-row">
        <span class="source-index" aria-hidden="true">{{ index + 1 }}</span>
        <div class="source-row-main">
          <div class="source-row-meta">
            <span>{{ t("订阅 {value}", { value: index + 1 }) }}</span>
            <span v-if="usageRows[index]?.hostname" class="source-host">{{ usageRows[index].hostname }}</span>
            <span v-if="usageRows[index]" class="source-state" :data-state="usageRows[index].state">{{ usageRows[index].stateLabel }}</span>
          </div>
          <input
            v-if="editable"
            class="source-row-input"
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
          <span v-else class="source-row-label" :title="sourceLabel(line)">{{ sourceLabel(line) || t("未填写") }}</span>
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
.source-list-heading, .source-list-title, .source-list-row, .source-row-meta, .local-source-row { display: flex; align-items: center; min-width: 0; }
.source-list-heading { justify-content: space-between; gap: 12px; }
.source-list-title { gap: 12px; }
.source-list-title > svg, .local-source-row > svg, .source-list-empty > svg { flex: 0 0 auto; color: var(--mn-ink-muted); }
.source-list-title h3 { margin: 0; color: var(--mn-ink); font-size: .9375rem; font-weight: 600; }
.source-list-title p { margin: 4px 0 0; color: var(--mn-ink-muted); font-size: .8125rem; line-height: 1.55; }
.source-list-rows { display: grid; gap: 8px; }
.source-list-row { gap: 10px; min-height: 64px; border: 1px solid var(--mn-border); border-radius: var(--mn-radius-sm); padding: 9px 10px; background: var(--mn-surface-sunken); }
.source-index { display: grid; place-items: center; flex: 0 0 auto; width: 28px; height: 28px; border: 1px solid var(--mn-border-strong); border-radius: 50%; color: var(--mn-ink-muted); font-size: .75rem; font-variant-numeric: tabular-nums; }
.source-row-main { display: grid; gap: 4px; min-width: 0; flex: 1; }
.source-row-meta { flex-wrap: wrap; gap: 3px 8px; color: var(--mn-ink-muted); font-size: .75rem; }
.source-host { color: var(--mn-ink-soft); overflow-wrap: anywhere; }
.source-state { color: var(--mn-ink-muted); }
.source-state[data-state="cached"] { color: var(--mn-warning); }
.source-row-label { color: var(--mn-ink-soft); font-size: .875rem; line-height: 1.5; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.source-row-input { width: 100%; min-width: 0; border: 0; border-bottom: 1px solid var(--mn-border-strong); padding: 3px 0; color: var(--mn-ink); background: transparent; font: inherit; font-size: .875rem; outline: none; }
.source-row-input:focus { border-bottom-color: var(--mn-primary); box-shadow: 0 1px 0 var(--mn-primary); }
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
  .source-row-label { white-space: normal; overflow-wrap: anywhere; }
}
</style>
