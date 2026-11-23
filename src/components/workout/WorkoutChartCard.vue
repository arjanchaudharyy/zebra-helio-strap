<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import type { DesignIconName } from '../DesignIcon.vue';
import GlyphTile from '../GlyphTile.vue';
import ElevationFacts from './ElevationFacts.vue';
import ShareBar, { type ShareSegment } from './ShareBar.vue';
import type { SlopeShare } from '../../lib/elevationProfile';
import type { ChartStat } from '../../composables/useWorkoutPresentation';
import type { GlyphTone } from '../../lib/glyphs';
import { CHART_THEME, VChart } from '../../lib/echartsSetup';
import { CHART_TYPE, chartTier, fitTicks, withDetail, withFontSize, type ChartDetail, type ChartTier, type lineOption } from '../../lib/workoutCharts';
import { useMessages } from '../../i18n';
import { axisPointerTime, setRouteCursor } from '../../composables/useRouteCursor';
import { workoutDetailMessages } from '../../views/WorkoutDetail.i18n';

const props = defineProps<{
  card: {
    key: string; title: string; unit: string; option: NonNullable<ReturnType<typeof lineOption>>; detail?: ChartDetail;
    stats: ChartStat[] | null; icon: DesignIconName; tone: GlyphTone;
    slope?: SlopeShare | null;
    zoneShare?: ShareSegment[] | null;
  };
}>();
const t = useMessages(workoutDetailMessages);
const root = ref<HTMLElement | null>(null);
const tier = ref<ChartTier>('sm');
const plotH = ref(0);
const option = computed(() => {
  const detailed = props.card.detail ? withDetail(props.card.option, tier.value, props.card.detail) : props.card.option;
  const sized = withFontSize(detailed, tier.value);
  return plotH.value ? fitTicks(sized, plotH.value - 40) : sized;
});
const showSlope = computed(() => tier.value === 'lg' && !!props.card.slope);
const showZones = computed(() => tier.value === 'lg' && !!props.card.zoneShare?.length);
const longStats = computed(() => (props.card.stats ?? []).some((stat) => stat.value.length > 5));
const typeVars = computed(() => {
  const type = CHART_TYPE[tier.value];
  return { '--chart-num': `${longStats.value ? type.numLong : type.num}px`, '--chart-label': `${type.label}px`, '--share-label': `${type.label}px`, '--share-value': `${type.label + 1.5}px` };
});

let observer: ResizeObserver | null = null;
onMounted(() => {
  observer = new ResizeObserver(([entry]) => {
    if (!entry) return;
    const next = chartTier(entry.contentRect.width, entry.contentRect.height);
    if (next !== tier.value) tier.value = next;
    const el = root.value?.querySelector<HTMLElement>('.series-chart');
    const h = el ? Math.round(el.clientHeight / 20) * 20 : 0;
    if (h && h !== plotH.value) plotH.value = h;
  });
  if (root.value) observer.observe(root.value);
});
onBeforeUnmount(() => {
  observer?.disconnect();
  setRouteCursor(null);
});

const onAxisPointer = (event: object) => setRouteCursor(axisPointerTime(event as Parameters<typeof axisPointerTime>[0]));
const clearCursor = () => setRouteCursor(null);
</script>

<template>
  <section ref="root" :class="['surface-card', 'chart-card', `tone-${card.tone}`, `tier-${tier}`]" :style="typeVars" :aria-label="card.title" @mouseleave="clearCursor">
    <div class="chart-head">
      <GlyphTile :name="card.icon" :tone="card.tone" :size="36" />
      <p class="card-title">{{ card.title }} <em>{{ card.unit }}</em></p>
    </div>
    <dl v-if="card.stats" :class="['chart-stats', { long: longStats }]" :style="{ '--stat-count': card.stats.length }">
      <div v-for="stat in card.stats" :key="stat.label"><dt>{{ stat.label }}</dt><dd>{{ stat.value }}</dd></div>
    </dl>
    <VChart class="series-chart" :key="CHART_THEME" :theme="CHART_THEME" :option="option" autoresize role="img" :aria-label="t.chartAria(card.title)" @update-axis-pointer="onAxisPointer" @globalout="clearCursor" />
    <ElevationFacts v-if="showSlope" class="chart-extra" :slope="card.slope ?? null" :facts="false" />
    <ShareBar v-if="showZones" class="chart-extra" :title="t.chartZoneShare" :segments="card.zoneShare!" />
  </section>
</template>

<style scoped>
.chart-card {
  --chart-tone: var(--accent);
  position: relative;
  display: flex;
  flex-direction: column;
  padding: 14px 16px 12px;
  border-radius: var(--radius-lg);
  container-type: inline-size;
}
.chart-card::before { position: absolute; inset: 0 18px auto; height: 2px; content: ''; border-radius: 0 0 2px 2px; background: linear-gradient(90deg, var(--chart-tone), transparent); }
.chart-card.tone-heart { --chart-tone: var(--heart); }
.chart-card.tone-pace { --chart-tone: var(--pace); }
.chart-card.tone-altitude { --chart-tone: var(--altitude); }
.chart-card.tone-activity { --chart-tone: var(--activity); }
.chart-head { display: flex; flex: none; align-items: center; gap: 10px; min-width: 0; }
.card-title { flex: 1 1 auto; min-width: 0; margin: 0; color: var(--ink); font-size: var(--fs-lg); font-weight: 700; }
.card-title em { color: var(--subtle); font-size: var(--fs-sm); font-style: normal; font-weight: 400; }
.chart-stats {
  display: grid;
  flex: none;
  grid-template-columns: repeat(var(--stat-count), minmax(0, 1fr));
  margin: 12px 0 6px;
  font-variant-numeric: tabular-nums;
}
.chart-stats div { display: flex; flex-direction: column-reverse; gap: 2px; min-width: 0; padding: 1px 12px; }
.chart-stats div:first-child { padding-left: 2px; }
.chart-stats div + div { border-left: 1px solid var(--mat-line); }
.chart-stats dt { overflow: hidden; color: var(--subtle); font-size: var(--chart-label, var(--fs-2xs)); text-overflow: ellipsis; white-space: nowrap; }
.chart-stats dd { margin: 0; color: var(--ink); font-family: 'Inter', var(--font-sans); font-size: var(--chart-num, var(--fs-3xl)); font-weight: 600; line-height: 1.2; white-space: nowrap; }
@container (max-width: 440px) {
  .chart-stats { grid-template-columns: repeat(min(var(--stat-count), 3), minmax(0, 1fr)); row-gap: 10px; }
  .chart-stats dd { font-size: min(var(--chart-num, var(--fs-xl)), var(--fs-xl)); }
  .chart-stats div:nth-child(3n + 1) { padding-left: 2px; border-left: 0; }
}
@container (max-width: 300px) {
  .chart-stats { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .chart-stats div:nth-child(3n + 1) { padding-left: 12px; border-left: 1px solid var(--mat-line); }
  .chart-stats div:nth-child(2n + 1) { padding-left: 2px; border-left: 0; }
}
.chart-extra { flex: none; margin: 10px 2px 2px; }
.series-chart { flex: 1 0 auto; width: 100%; height: max(120px, calc(clamp(170px, 34cqw, 300px) + var(--fill-extra, 0px))); }
</style>
