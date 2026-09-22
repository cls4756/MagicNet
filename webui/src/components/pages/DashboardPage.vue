<script setup lang="ts">
import ControlPage from "./ControlPage.vue";
import AboutPage from "./AboutPage.vue";
import GroupSection from "@/components/ui/GroupSection.vue";

const emit = defineEmits<{
  (e: "goto-output"): void;
  (e: "goto-tab", tab: string): void;
}>();
</script>

<template>
  <div class="mn-settings-stack">
    <ControlPage
      @goto-output="emit('goto-output')"
      @goto-tab="(tab: string) => emit('goto-tab', tab)"
    />
    <GroupSection>
      <AboutPage @goto-tab="(tab: string) => emit('goto-tab', tab)" />
    </GroupSection>
  </div>
</template>
