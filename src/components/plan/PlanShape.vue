<script setup lang="ts">
import { computed } from 'vue';
import type { WorkoutProfile } from '../../lib/trainingPlan/profile';
import { segmentLevel, type HrDomain } from '../../lib/trainingPlan/summary';
import { INTENSITY_COLOR } from '../../lib/trainingPlan/intensity';

const props = defineProps<{ profile: WorkoutProfile; scaleSeconds: number; domain: HrDomain | null }>();

const W = 120;
const H = 30;
const GAP = 1.2;

const bars = computed(() => {
  const unit = W / Math.max(props.scaleSeconds, props.profile.seconds, 1);
  let at = 0;
  return props.profile.segments.map((segment, index) => {
    const width = Math.max(1.6, segment.seconds * unit - GAP);
    const x = at;
    at += segment.seconds * unit;
    const height = Math.max(3, segmentLevel(segment, props.domain) * H);
    return {
      index,
      x,
      y: H - height,
      width,
      height,
      color: INTENSITY_COLOR[segment.intensity],
      soft: segment.low === null,
      approx: segment.approx,
    };
  });
});
</script>

<template>
  <svg class="shape" :viewBox="`0 0 ${W} ${H}`" preserveAspectRatio="none" aria-hidden="true">
    <rect v-for="bar in bars" :key="bar.index" :class="{ soft: bar.soft, approx: bar.approx }" :x="bar.x" :y="bar.y"
      :width="bar.width" :height="bar.height" :rx="Math.min(2, bar.width / 2)" :fill="bar.color" />
  </svg>
</template>

<style scoped>
.shape { display: block; width: 100%; height: 30px; overflow: visible; }
rect { opacity: .92; }
rect.soft { opacity: .45; }
rect.approx { opacity: .55; }
</style>
