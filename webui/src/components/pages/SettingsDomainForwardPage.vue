<script setup lang="ts">
import { t } from "@/i18n";
import PageHeader from "@/components/ui/PageHeader.vue";
import DomainForwardCard from "./DomainForwardCard.vue";
</script>

<template>
  <div class="grid gap-4">
    <PageHeader :overline="t('设置')" :title="t('域名转发')" />
    <DomainForwardCard />
  </div>
</template>