<script setup lang="ts">
import { Loader2 } from "lucide-vue-next";
import { cva } from "class-variance-authority";
import { computed, ref, useAttrs } from "vue";
import type { ClassValue } from "clsx";
import { t } from "@/i18n";
import { cn } from "@/lib/utils";

defineOptions({ inheritAttrs: false });

const props = withDefaults(
  defineProps<{
    variant?: "default" | "secondary" | "outline" | "ghost" | "destructive";
    size?: "sm" | "md" | "icon";
    loading?: boolean;
    autoLoading?: boolean;
    disabled?: boolean;
    type?: "button" | "submit" | "reset";
    class?: ClassValue;
  }>(),
  {
    variant: "default",
    size: "md",
    loading: false,
    autoLoading: true,
    disabled: false,
    type: "button",
  },
);

type ClickHandler = (event: MouseEvent) => unknown;

const attrs = useAttrs();
const pending = ref(false);
const busy = computed(() => props.loading || pending.value);
const forwardedAttrs = computed(() => {
  const { onClick: _onClick, ...rest } = attrs;
  return rest;
});

function isPromiseLike(value: unknown): value is PromiseLike<unknown> {
  return (
    (typeof value === "object" && value !== null) || typeof value === "function"
  ) && typeof (value as PromiseLike<unknown>).then === "function";
}

function clickHandlers(): ClickHandler[] {
  const listener = attrs.onClick;
  const listeners = Array.isArray(listener) ? listener : [listener];
  return listeners.filter((item): item is ClickHandler => typeof item === "function");
}

function handleClick(event: MouseEvent): void {
  if (busy.value || props.disabled) {
    event.preventDefault();
    event.stopImmediatePropagation();
    return;
  }

  const tasks: PromiseLike<unknown>[] = [];
  for (const handler of clickHandlers()) {
    const result = handler(event);
    if (props.autoLoading && isPromiseLike(result)) tasks.push(result);
  }
  if (!tasks.length) return;

  pending.value = true;
  void Promise.allSettled(tasks.map((task) => Promise.resolve(task))).then(() => {
    pending.value = false;
  });
}

const buttonVariants = cva(
  "mn-button group relative inline-flex max-w-full items-center justify-center whitespace-normal rounded-[var(--mn-radius-md)] border text-sm font-medium transition-[transform,color,background-color,border-color,opacity] duration-150 ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--mn-focus)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--mn-ivory)] active:translate-y-px disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-55 disabled:active:translate-y-0",
  {
    variants: {
      variant: {
        default:
          "border-[var(--mn-primary)] bg-[var(--mn-primary)] text-[var(--mn-on-accent)] hover:border-[var(--mn-primary-strong)] hover:bg-[var(--mn-primary-strong)]",
        secondary:
          "border-transparent bg-[var(--mn-surface-sunken)] text-[var(--mn-ink)] hover:border-[var(--mn-primary)] hover:text-[var(--mn-primary-strong)]",
        outline:
          "border-[var(--mn-border-strong)] bg-transparent text-[var(--mn-ink)] hover:border-[var(--mn-primary)] hover:bg-[color-mix(in_srgb,var(--mn-primary)_9%,transparent)]",
        ghost:
          "border-transparent bg-transparent text-[var(--mn-ink-muted)] hover:border-[var(--mn-border)] hover:bg-[var(--mn-surface-sunken)] hover:text-[var(--mn-ink)]",
        destructive:
          "border-[var(--mn-danger)] bg-[var(--mn-danger)] text-[var(--mn-on-danger)] hover:brightness-110",
      },
      size: {
        sm: "min-h-12 px-3.5",
        md: "min-h-12 px-4",
        icon: "size-12 shrink-0 p-0",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "md",
    },
  },
);

const classes = computed(() =>
  cn(buttonVariants({ variant: props.variant, size: props.size }), props.class),
);
</script>

<template>
  <button
    v-bind="forwardedAttrs"
    :type="type"
    :class="classes"
    :data-size="size"
    :disabled="busy || disabled"
    :aria-busy="busy ? 'true' : undefined"
    @click="handleClick"
  >
    <span v-if="busy" class="mn-button__busy" aria-live="polite">
      <Loader2 class="motion-safe:animate-spin" :size="18" aria-hidden="true" />
      <span v-if="size !== 'icon'">{{ t("执行中…") }}</span>
    </span>
    <span class="mn-button__content inline-flex min-w-0 items-center justify-center gap-2">
      <slot />
    </span>
  </button>
</template>

<style scoped>
/* Busy feedback overlays the stable button geometry, then restores the label. */
.mn-button[aria-busy="true"] { opacity: 1; }
.mn-button__busy {
  position: absolute;
  inset: 0;
  display: inline-flex;
  min-width: 0;
  align-items: center;
  justify-content: center;
  gap: 0.5rem;
  padding-inline: 0.75rem;
  white-space: nowrap;
  pointer-events: none;
}
.mn-button[aria-busy="true"] .mn-button__content { visibility: hidden; }
</style>
