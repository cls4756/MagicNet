<script setup lang="ts">
import GroupSection from "@/components/ui/GroupSection.vue";
import SettingsStackPage from "../SettingsStackPage.vue";
import SettingsDomainForwardPage from "../SettingsDomainForwardPage.vue";
import BlocklistPage from "../BlocklistPage.vue";
</script>

<template>
  <GroupSection>
    <div class="mn-settings-stack">
      <SettingsStackPage />
      <SettingsDomainForwardPage />
      <BlocklistPage />
    </div>
  </GroupSection>
</template>
