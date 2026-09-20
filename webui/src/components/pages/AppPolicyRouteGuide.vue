<script setup lang="ts">
import { t } from "@/i18n";
import { ArrowRight, ChevronDown, ChevronUp, Info, RefreshCw, X } from "lucide-vue-next";
import { computed, nextTick, onUnmounted, ref, watch } from "vue";
import Button from "@/components/ui/Button.vue";
import { restoreFocusAfterUpdate, trapFocusWithin } from "@/lib/focus";
import {
  appPolicyModeSummary,
  appPolicyRouteDefinitions,
  type AppPolicyMode,
} from "./appPolicyRouteModel.ts";

const props = withDefaults(
  defineProps<{
    mode: AppPolicyMode;
    reapplyLoading?: boolean;
  }>(),
  { reapplyLoading: false },
);

const emit = defineEmits<{ reapply: [] }>();

const open = ref(false);
const expanded = ref(false);
const dialog = ref<HTMLElement | null>(null);
const modeSummary = computed(() => appPolicyModeSummary(props.mode));
let previousBodyOverflow = "";
let trigger: EventTarget | null = null;

watch(open, (value) => {
  if (value) {
    previousBodyOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return;
  }
  document.body.style.overflow = previousBodyOverflow;
  if (trigger) restoreFocusAfterUpdate(trigger);
  trigger = null;
});

onUnmounted(() => {
  if (open.value) document.body.style.overflow = previousBodyOverflow;
});

function openDialog(event: Event): void {
  trigger = event.currentTarget;
  open.value = true;
  void nextTick(() => {
    dialog.value?.querySelector<HTMLElement>("[data-dialog-initial-focus]")?.focus();
  });
}

function closeDialog(): void {
  open.value = false;
  expanded.value = false;
}

function trapFocus(event: KeyboardEvent): void {
  trapFocusWithin(event, dialog.value);
}

/** The confirmation panel owns the restart and the focus, so the guide closes first. */
function requestReapply(): void {
  trigger = null;
  closeDialog();
  emit("reapply");
}
</script>

<template>
  <div class="flex flex-wrap items-center justify-between gap-2 rounded-[var(--mn-radius-md)] border border-[var(--mn-border)] bg-[var(--mn-surface-sunken)] px-3 py-2">
    <p class="flex min-w-0 items-start gap-2 text-xs leading-5 text-[var(--mn-ink-muted)]">
      <Info :size="15" class="mt-0.5 shrink-0" aria-hidden="true" />
      <span>{{ t('不确定应用流量会走代理、直连还是绕过？') }}</span>
    </p>
    <Button variant="ghost" size="sm" aria-haspopup="dialog" @click="openDialog">
      <ChevronDown :size="15" />{{ t('流量如何处理？') }}
    </Button>
  </div>

  <Teleport to="body">
    <Transition name="sheet">
      <div v-if="open" class="fixed inset-0 z-[70] grid place-items-end p-3 sm:place-items-center sm:p-6" role="presentation">
        <button
          class="mn-overlay absolute inset-0 size-full"
          type="button"
          :aria-label="t('关闭对话框')"
          @click="closeDialog"
        />
        <section
          ref="dialog"
          class="mn-chrome relative z-10 grid max-h-[calc(100dvh-1.5rem)] w-full max-w-3xl gap-3 overflow-y-auto rounded-md p-1.5"
          role="dialog"
          aria-modal="true"
          aria-labelledby="app-route-guide-title"
          tabindex="-1"
          @keydown="trapFocus"
          @keydown.esc.prevent.stop="closeDialog"
        >
          <div class="grid gap-3 rounded-[5px] bg-[var(--mn-ivory)] p-4 sm:p-5">
            <div class="flex items-start justify-between gap-3">
              <div class="min-w-0">
                <h2 id="app-route-guide-title" class="text-lg font-semibold text-[var(--mn-ink)]">{{ t('流量如何处理？') }}</h2>
                <p class="mt-1 text-sm leading-6 text-[var(--mn-ink-muted)]">{{ modeSummary }}</p>
              </div>
              <Button data-dialog-initial-focus variant="ghost" size="icon" :aria-label="t('关闭对话框')" @click="closeDialog">
                <X :size="18" />
              </Button>
            </div>

            <div id="app-route-guide-details" class="grid gap-px overflow-hidden rounded-[var(--mn-radius-md)] border border-[var(--mn-border)] bg-[var(--mn-border)] lg:grid-cols-3">
              <article
                v-for="route in appPolicyRouteDefinitions"
                :key="route.id"
                class="grid min-w-0 gap-2 bg-[var(--mn-surface-raised)] p-3"
              >
                <strong class="text-sm">{{ route.label }}</strong>
                <div class="flex min-w-0 flex-wrap items-center gap-1 text-xs text-[var(--mn-ink-soft)]" :aria-label="t('{value} 流量路径：{value2}', { value: route.label, value2: route.steps.map((step) => t(step)).join(t(' 到 ')) })">
                  <template v-for="(step, index) in route.steps" :key="`${route.id}-${step}`">
                    <span class="rounded border border-[var(--mn-border)] bg-[var(--mn-surface-sunken)] px-1.5 py-1 font-mono">{{ t(step) }}</span>
                    <ArrowRight v-if="index < route.steps.length - 1" class="shrink-0 text-[var(--mn-ink-faint)]" :size="13" aria-hidden="true" />
                  </template>
                </div>
                <span class="text-xs text-[var(--mn-ink-muted)]">DNS：{{ t(route.dnsShort) }}</span>
                <div v-if="expanded" class="grid gap-1 border-t border-[var(--mn-border)] pt-2 text-xs leading-5 text-[var(--mn-ink-muted)]">
                  <p><strong class="text-[var(--mn-ink-soft)]">{{ t('数据：') }}</strong>{{ t(route.traffic) }}</p>
                  <p><strong class="text-[var(--mn-ink-soft)]">DNS：</strong>{{ t(route.dns) }}</p>
                  <p><strong class="text-[var(--mn-ink-soft)]">{{ t('适合：') }}</strong>{{ t(route.useCase) }}</p>
                </div>
              </article>
            </div>

            <div class="flex justify-end">
              <Button
                variant="ghost"
                size="sm"
                :aria-expanded="expanded"
                aria-controls="app-route-guide-details"
                @click="expanded = !expanded"
              >
                <ChevronUp v-if="expanded" :size="15" />
                <ChevronDown v-else :size="15" />
                {{ expanded ? t('收起详细差异') : t('查看详细差异') }}
              </Button>
            </div>

            <div class="grid gap-3 rounded-[var(--mn-radius-md)] border border-[var(--mn-border)] bg-[var(--mn-surface-raised)] p-3 sm:flex sm:flex-wrap sm:items-center sm:justify-between">
              <p class="min-w-0 flex-1 text-xs leading-5 text-[var(--mn-ink-muted)]">
                {{ t('名单修改会自动解析 UID、套用配置并重启当前核心。只有应用重装、新增工作资料／Android 用户或 UID 改变后，才需要手动重新解析。') }}
              </p>
              <Button variant="outline" size="sm" :loading="reapplyLoading" @click="requestReapply">
                <RefreshCw :size="15" />{{ t('重新解析 App UID') }}
              </Button>
            </div>
          </div>
        </section>
      </div>
    </Transition>
  </Teleport>
</template>
