<script setup lang="ts">
import GlyphTile from '../GlyphTile.vue';
import WorkoutMuscleMap from './WorkoutMuscleMap.vue';
import { useMessages } from '../../i18n';
import { workoutStrengthMessages } from './WorkoutStrengthCard.i18n';
import type { StrengthDetail } from '../../composables/useStrengthDetail';

const props = defineProps<{ detail: StrengthDetail }>();
const t = useMessages(workoutStrengthMessages);
const { load, mapRow, mapShape, mapShapes, hoverOn, pin } = props.detail;
</script>

<template>
  <section class="surface-card strength-map-card" :aria-label="t.muscleShareAria">
    <div class="section-head">
      <GlyphTile name="power" tone="training" :size="36" />
      <div>
        <p class="section-eyebrow">{{ t.mapEyebrow }}</p>
        <h2>{{ t.mapTitle }}</h2>
      </div>
    </div>
    <WorkoutMuscleMap
      class="muscle-figure"
      :load="load"
      :active-row="mapRow"
      :active-shape="mapShape"
      :active-shapes="mapShapes"
      @hover-shape="hoverOn('shape', $event)"
      @pin-shape="pin('shape', $event)"
    />
  </section>
</template>

<style scoped>
.strength-map-card { display: grid; gap: 12px; padding: 16px 16px 14px; border-radius: var(--radius-lg); container-type: inline-size; }
.section-head { display: flex; align-items: center; gap: 10px; }
.section-head h2 { margin: 1px 0 0; font-size: var(--fs-xl); letter-spacing: -.02em; }
.section-eyebrow { margin: 0; color: var(--subtle); font-family: var(--font-mono); font-size: var(--fs-2xs); font-weight: 700; letter-spacing: .16em; text-transform: uppercase; }
.muscle-figure { padding: 10px 6px 8px; border-radius: var(--radius-md); background: var(--mat-inset); }
.muscle-figure :deep(.figure) { max-width: 200px; }
@container (max-width: 320px) {
  .strength-map-card { padding: 8px; gap: 0; }
  .section-head { display: none; }
}
</style>
