<script setup lang="ts">
import GroupSection from "@/components/ui/GroupSection.vue";
import ConfigPage from "../ConfigPage.vue";
import WebuiPage from "../WebuiPage.vue";
</script>

<template>
  <GroupSection>
    <div class="mn-settings-stack">
      <ConfigPage />
      <WebuiPage />
    </div>
  </GroupSection>
</template>
