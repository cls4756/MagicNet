<script setup lang="ts">
import GroupSection from "@/components/ui/GroupSection.vue";
import ToolsPage from "../ToolsPage.vue";
import DiagnosticsPage from "../DiagnosticsPage.vue";
import TerminalPage from "../TerminalPage.vue";
</script>

<template>
  <GroupSection>
    <div class="mn-settings-stack">
      <DiagnosticsPage />
      <ToolsPage />
      <TerminalPage />
    </div>
  </GroupSection>
</template>
