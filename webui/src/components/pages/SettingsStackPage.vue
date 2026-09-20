<script setup lang="ts">
import { t } from "@/i18n";
import PageHeader from "@/components/ui/PageHeader.vue";
import NetworkPolicyCard from "./NetworkPolicyCard.vue";
</script>

<template>
  <div class="grid gap-4">
    <PageHeader :overline="t('设置')" :title="t('协议栈')" />
    <NetworkPolicyCard />
  </div>
</template>