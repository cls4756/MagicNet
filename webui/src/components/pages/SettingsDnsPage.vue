<script setup lang="ts">
import { t } from "@/i18n";
import PageHeader from "@/components/ui/PageHeader.vue";
import DnsToolsCard from "./DnsToolsCard.vue";
</script>

<template>
  <div class="grid gap-4">
    <PageHeader :overline="t('设置')" :title="t('DNS 配置')" />
    <DnsToolsCard />
  </div>
</template>