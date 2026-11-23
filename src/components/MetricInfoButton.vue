<script setup lang="ts">
import { computed, ref } from 'vue';
import Icon from './Icon.vue';
import GlassPopover from './GlassPopover.vue';
import { metricInfo } from '../lib/metricInfo';
import { metricTrust } from '../lib/metricInfo/trust';
import GlassSwitch from './GlassSwitch.vue';
import { isDesktop } from '../lib/bridge';
import { useMetricExclusions } from '../composables/useMetricExclusions';
import { useMetricInfoText } from './MetricInfo.i18n';

const props = defineProps<{
  metric: string;
  label: string;
}>();

const t = useMetricInfoText();
const info = computed(() => metricInfo(props.metric));
const trust = computed(() => metricTrust(props.metric));
const { isExcluded, setExcluded } = useMetricExclusions();
const excluded = computed(() => isExcluded(props.metric));
const toggleUse = (use: boolean) => { void setExcluded(props.metric, !use).catch(() => undefined); };
const trustText = computed(() => {
  if (trust.value === 'measured') return t.value.trustMeasured;
  if (trust.value === 'computed') return t.value.trustComputed;
  if (trust.value === 'inferred') return t.value.trustInferred;
  return t.value.trustEstimated;
});
const flag = computed(() => (excluded.value ? 'excluded' : trust.value === 'modeled' ? 'modeled' : null));
const buttonLabel = computed(() => {
  const about = t.value.about(props.label);
  if (flag.value === 'excluded') return `${about} · ${t.value.excludedChipTitle}`;
  if (flag.value === 'modeled') return `${about} · ${t.value.trustChipTitle}`;
  return about;
});
const open = ref(false);
const button = ref<HTMLElement | null>(null);
const titleId = computed(() => `metric-info-${props.metric}`);
</script>

<template>
  <button
    v-if="info"
    ref="button"
    type="button"
    class="metric-info"
    data-tour="metric-info"
    :data-flag="flag ?? undefined"
    :title="buttonLabel"
    :aria-label="buttonLabel"
    aria-haspopup="dialog"
    :aria-expanded="open"
    @click="open = true"
  >
    <Icon name="help" :size="15" />
  </button>
  <GlassPopover
    v-if="open && info"
    :anchor="button"
    :labelledby="titleId"
    @close="open = false"
  >
    <h2 :id="titleId" class="info-title" data-pop-item>{{ label }}</h2>
    <section class="info-section" data-pop-item>
      <h3>{{ t.sectionWhat }}</h3>
      <p>{{ info.what }}</p>
    </section>
    <section class="info-section" data-pop-item>
      <h3>{{ t.sectionChart }}</h3>
      <p>{{ info.chart }}</p>
    </section>
    <section class="info-section" data-pop-item>
      <h3>{{ t.sectionHow }}</h3>
      <p>{{ info.how }}</p>
    </section>
    <section v-if="trust" class="info-section" data-pop-item>
      <h3>{{ t.sectionTrust }}</h3>
      <p>{{ trustText }}</p>
    </section>
    <section v-if="isDesktop()" class="info-section use-row" data-pop-item>
      <div class="use-copy">
        <h3>{{ t.useInAnalysis }}</h3>
        <p>{{ t.useInAnalysisHint }}</p>
      </div>
      <GlassSwitch :model-value="!excluded" :aria-label="t.useInAnalysis" @update:model-value="toggleUse" />
    </section>
  </GlassPopover>
</template>

<style scoped>
.metric-info {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border: 0;
  border-radius: 999px;
  background: var(--mat-inset);
  box-shadow: var(--mat-inset-shadow);
  color: var(--muted);
  font: inherit;
  cursor: pointer;
  vertical-align: middle;
  transition: background var(--dur-base) ease, color var(--dur-base) ease, transform 160ms ease;
}
.metric-info:hover { background: color-mix(in srgb, var(--ink) 8%, var(--mat-inset)); color: var(--ink); }
.metric-info:active { transform: scale(.92); }
.metric-info[aria-expanded='true'] { color: var(--accent); background: color-mix(in srgb, var(--accent) 14%, var(--mat-inset)); }
.metric-info:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }

.metric-info { position: relative; }
.metric-info[data-flag]::after {
  content: '';
  position: absolute;
  top: 1px;
  right: 1px;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  box-shadow: 0 0 0 2px var(--mat-card-solid);
  pointer-events: none;
}
.metric-info[data-flag='modeled']::after { background: var(--warning); }
.metric-info[data-flag='excluded']::after {
  background: linear-gradient(135deg, transparent 42%, var(--mat-card-solid) 42% 58%, transparent 58%), var(--subtle);
}
.use-row { display: flex; align-items: center; gap: 12px; }
.use-copy { flex: 1; min-width: 0; }

.info-title { margin: 0; color: var(--ink); font-size: var(--fs-md); line-height: 1.3; }
.info-section h3 {
  margin: 0 0 2px;
  color: var(--subtle);
  font-size: var(--fs-2xs);
  font-weight: 600;
  letter-spacing: 0.02em;
}
.info-section p { margin: 0; color: var(--muted); font-size: var(--fs-sm); line-height: 1.55; }
</style>
