<script setup lang="ts">
import { provide } from "vue";

// Renders slotted pages with their PageHeader collapsed into a compact section
// label, so several existing pages can be stacked inside one merged page.
provide("mnHeaderMode", "group");
</script>

<template>
  <div class="mn-group-section min-w-0">
    <slot />
  </div>
</template>
