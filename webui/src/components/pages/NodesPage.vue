<script setup lang="ts">
import ProxyPage from "./ProxyPage.vue";
import ProxyChainPage from "./ProxyChainPage.vue";
import GroupSection from "@/components/ui/GroupSection.vue";
</script>

<template>
  <div class="mn-settings-stack">
    <ProxyPage />
    <GroupSection>
      <ProxyChainPage />
    </GroupSection>
  </div>
</template>
