<script setup lang="ts">
import { t } from "@/i18n";
import PageHeader from "@/components/ui/PageHeader.vue";
import ProxyGroupsPanel from "./ProxyGroupsPanel.vue";
import RoutingRulesPanel from "./RoutingRulesPanel.vue";
</script>

<template>
  <div class="proxy-page">
    <PageHeader
      :overline="t('运行策略')"
      :title="t('节点与路由')"
      :description="t('在这里选择策略组出站，并查看当前生效的路由规则。')"
    />
    <ProxyGroupsPanel />
    <RoutingRulesPanel />
  </div>
</template>

<style scoped>
.proxy-page { display: grid; width: 100%; max-width: 1120px; min-width: 0; gap: 20px; margin: 0 auto; }
</style>
