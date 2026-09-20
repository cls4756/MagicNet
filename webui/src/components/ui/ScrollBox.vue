<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, onUpdated, ref } from "vue";
import { cn } from "@/lib/utils";
import { scrollRailGeometry, type ScrollMetrics } from "@/lib/scrollRail";

const props = defineProps<{ class?: string }>();
const classes = computed(() => cn("mn-scroll-box__viewport min-w-0", props.class));
const viewport = ref<HTMLElement | null>(null);
const metrics = ref<ScrollMetrics>({ scrollTop: 0, scrollHeight: 0, clientHeight: 0 });
const nativeRail = ref(false);

/** Platforms that reserve space for the scrollbar already show it; overlay ones do not. */
const rail = computed(() =>
  nativeRail.value
    ? { visible: false, height: 0, offset: 0 }
    : scrollRailGeometry(metrics.value),
);
const thumbStyle = computed(() => ({
  height: `${rail.value.height}px`,
  transform: `translateY(${rail.value.offset}px)`,
}));

function sync(): void {
  const element = viewport.value;
  if (!element) return;
  const style = window.getComputedStyle(element);
  const border = (parseFloat(style.borderLeftWidth) || 0) + (parseFloat(style.borderRightWidth) || 0);
  nativeRail.value = element.offsetWidth - element.clientWidth - border > 1;
  const next: ScrollMetrics = {
    scrollTop: element.scrollTop,
    scrollHeight: element.scrollHeight,
    clientHeight: element.clientHeight,
  };
  const current = metrics.value;
  // Writing an equal object back would re-render and re-enter sync forever.
  if (
    current.scrollTop !== next.scrollTop ||
    current.scrollHeight !== next.scrollHeight ||
    current.clientHeight !== next.clientHeight
  ) {
    metrics.value = next;
  }
}

let observer: ResizeObserver | undefined;
onMounted(() => {
  sync();
  if (typeof ResizeObserver === "function" && viewport.value) {
    observer = new ResizeObserver(sync);
    observer.observe(viewport.value);
  }
});
onUpdated(() => void nextTick(sync));
onBeforeUnmount(() => observer?.disconnect());
</script>

<template>
  <div class="mn-scroll-box">
    <div ref="viewport" :class="classes" @scroll="sync">
      <slot />
    </div>
    <div v-if="rail.visible" class="mn-scroll-box__rail" aria-hidden="true">
      <div class="mn-scroll-box__thumb" :style="thumbStyle" />
    </div>
  </div>
</template>
