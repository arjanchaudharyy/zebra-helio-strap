<script setup lang="ts">
import { computed } from 'vue';

export type GlyphCell = readonly [number, number, number, number];

const props = defineProps<{ cells: readonly GlyphCell[] }>();

const W = 30;
const H = 20;
const PAD = 2.5;
const GAP = 1.5;
const rects = computed(() => props.cells.map(([x, y, w, h]) => {
  const iw = W - PAD * 2 + GAP;
  const ih = H - PAD * 2 + GAP;
  return { x: PAD + x * iw, y: PAD + y * ih, w: w * iw - GAP, h: h * ih - GAP };
}));
</script>

<template>
  <svg class="layout-glyph" :viewBox="`0 0 ${W} ${H}`" :width="W" :height="H" aria-hidden="true" focusable="false">
    <rect class="glyph-frame" x=".75" y=".75" :width="W - 1.5" :height="H - 1.5" rx="4" />
    <rect v-for="(r, i) in rects" :key="i" class="glyph-cell" :x="r.x" :y="r.y" :width="r.w" :height="r.h" rx="1.4" />
  </svg>
</template>

<style scoped>
.layout-glyph { display: block; flex: none; }
.glyph-frame { fill: none; stroke: currentColor; stroke-width: 1.5; opacity: .55; }
.glyph-cell { fill: currentColor; }
</style>
