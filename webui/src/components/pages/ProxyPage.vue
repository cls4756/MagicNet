<script setup lang="ts">
import { t } from "@/i18n";
import { ref } from "vue";
import PageHeader from "@/components/ui/PageHeader.vue";
import ProxyGroupsPanel from "./ProxyGroupsPanel.vue";
import RoutingRulesPanel from "./RoutingRulesPanel.vue";
const groupsPanel = ref<{ refreshGroups: () => Promise<void> } | null>(null);
</script>

<template>
  <div class="proxy-page">
    <PageHeader
      :overline="t('运行策略')"
      :title="t('节点与路由')"
      :description="t('直接选择节点或策略组，管理可复用的用户策略与路由规则。')"
    />
    <ProxyGroupsPanel ref="groupsPanel" />
    <RoutingRulesPanel @applied="groupsPanel?.refreshGroups()" />
  </div>
</template>

<style scoped>
.proxy-page { display: grid; width: 100%; max-width: 1120px; min-width: 0; gap: 20px; margin: 0 auto; }
</style>
