<script setup lang="ts">
import { t } from "@/i18n";
import PageHeader from "@/components/ui/PageHeader.vue";
import ProxyGroupsPanel from "./ProxyGroupsPanel.vue";
</script>

<template>
  <div class="proxy-page">
    <PageHeader
      :overline="t('运行策略')"
      :title="t('代理')"
      :description="t('订阅负责提供来源，代理页面按策略组展示实际节点并切换当前节点。')"
    />
    <ProxyGroupsPanel />
  </div>
</template>

<style scoped>
.proxy-page { display: grid; width: 100%; max-width: 1120px; min-width: 0; gap: 20px; margin: 0 auto; }
</style>
