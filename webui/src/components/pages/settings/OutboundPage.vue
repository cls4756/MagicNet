<script setup lang="ts">
import GroupSection from "@/components/ui/GroupSection.vue";
import SettingsWarpPage from "../SettingsWarpPage.vue";
import TailscalePage from "../TailscalePage.vue";
</script>

<template>
  <GroupSection>
    <div class="mn-settings-stack">
      <SettingsWarpPage />
      <TailscalePage />
    </div>
  </GroupSection>
</template>
