<script setup lang="ts">
import { inject } from "vue";
import { t } from "@/i18n";

defineProps<{
  overline?: string;
  title: string;
  description?: string;
}>();

// Merged settings pages provide "group" so each embedded page renders its
// title as a compact section label instead of a full page header.
const mode = inject<"page" | "group">("mnHeaderMode", "page");
</script>

<template>
  <div v-if="mode === 'group'" class="mn-group-header">
    <div class="mn-group-header-row">
      <h3 class="mn-group-label">{{ title }}</h3>
      <div v-if="$slots.default || $slots.actions" class="mn-group-actions">
        <slot name="actions">
          <slot />
        </slot>
      </div>
    </div>
    <details v-if="description" class="mn-page-note">
      <summary>{{ t("说明") }}</summary>
      <p>{{ description }}</p>
    </details>
  </div>

  <div v-else class="mn-page-header">
    <div class="mn-page-heading min-w-0 max-w-3xl">
      <h2 class="text-[var(--mn-ink)]">{{ title }}</h2>
      <details v-if="description" class="mn-page-note">
        <summary>{{ t("说明") }}</summary>
        <p>{{ description }}</p>
      </details>
    </div>
    <div v-if="$slots.default || $slots.actions" class="mn-page-actions md:justify-end">
      <slot name="actions">
        <slot />
      </slot>
    </div>
  </div>
</template>
