<script setup lang="ts">
import { computed, ref } from 'vue';
import { useRouter } from 'vue-router';
import Icon from '../Icon.vue';
import GlassPopover from '../GlassPopover.vue';
import { askPresets, daysEndingOn, sleepAskPresets, workoutAskPresets, type AskPreset } from '../../lib/metricAsk';
import { AI_TASK_CATEGORY_META } from '../../lib/aiTask/categories';
import { dayKey } from '../../lib/aiTask/bridgeScale';
import type { AiTaskCategory } from '../../lib/bridge/types';
import { useAiTaskDraft } from '../../composables/useAiTaskDraft';
import { useMetricBaselines } from '../../composables/useMetricBaselines';
import { armPageMorph } from '../../composables/usePageMorph';
import { useAskText } from './ask.i18n';

const props = defineProps<{
  metric?: string | null;
  workout?: { id: string; label: string } | null;
  night?: { date: string } | null;
  label: string;
  card?: string;
  direct?: boolean;
  busy?: boolean;
  expanded?: boolean;
}>();
const emit = defineEmits<{ activate: [] }>();
const a = useAskText();
const router = useRouter();
const ctl = useAiTaskDraft();
const { baselineOf } = useMetricBaselines();
const open = ref(false);
const button = ref<HTMLElement | null>(null);
const popover = ref<InstanceType<typeof GlassPopover> | null>(null);
const direction = computed(() => baselineOf(props.metric)?.direction ?? null);
const titleId = computed(() => `ask-${props.workout ? `workout-${props.workout.id}` : props.night ? `night-${props.night.date}` : props.metric ?? 'metric'}`);
const shown = computed(() => !!props.direct || !!props.metric || !!props.workout || !!props.night);
const expandedNow = computed(() => (props.direct ? !!props.expanded : open.value));
const onPress = () => {
  if (props.busy) return;
  if (props.direct) { emit('activate'); return; }
  show();
};
defineExpose({
  get button(): HTMLElement | null { return button.value; },
});
const presets = computed(() => (props.workout ? workoutAskPresets() : props.night ? sleepAskPresets() : props.metric ? askPresets(props.metric) : []));
const questions = computed(() => presets.value.map((preset) => ({ preset, text: questionText(preset) })));
function questionText(preset: AskPreset): string {
  const t = a.value;
  switch (preset.key) {
    case 'first': return direction.value === 'above' ? t.firstAbove(props.label) : direction.value === 'below' ? t.firstBelow(props.label) : t.firstUsual(props.label);
    case 'withTraining': return t.withTraining(props.label);
    case 'withSleep': return t.withSleep;
    case 'sleepWithTraining': return t.sleepWithTraining;
    case 'trainingNext': return t.trainingNext;
    case 'bodyFood': return t.bodyFood(props.label);
    case 'workoutReview': return t.workoutReview(props.label);
    case 'workoutRecovery': return t.workoutRecovery(props.label);
    case 'workoutNext': return t.workoutNext(props.label);
    case 'sleepNight': return t.sleepNight;
    case 'sleepRecovery': return t.sleepRecovery;
    default: return t.firstUsual(props.label);
  }
}
const show = () => {
  open.value = true;
  void import('../../views/AiComposer.vue').catch(() => undefined);
};
const sourceCard = (): HTMLElement | null => {
  if (props.card) return document.getElementById('main-content')?.querySelector<HTMLElement>(props.card) ?? null;
  return button.value?.closest<HTMLElement>('.workout-hero, .trend-card, .surface-card, section') ?? null;
};
const choose = async (preset: AskPreset, text: string) => {
  const daysOf = (category: AiTaskCategory) => preset.daysBy?.[category] ?? preset.days;
  ctl.resetDraft();
  for (const range of ctl.draft.value.categories) {
    if (!AI_TASK_CATEGORY_META[range.category].hasWindow) continue;
    ctl.setCategoryEnabled(range.category, preset.categories.includes(range.category));
  }
  ctl.setWindowDays(preset.days - 1);
  for (const category of preset.categories) if (daysOf(category) !== preset.days) ctl.setCategoryDays(category, daysOf(category) - 1);
  if (preset.detail) ctl.setDetailLevel(preset.detail);
  if (props.workout) ctl.setWorkoutSelected(props.workout.id, true);
  const today = dayKey();
  if (props.night && props.night.date < today) {
    ctl.applyPicks(new Map(preset.categories.map((category) => [category, daysEndingOn(props.night!.date, daysOf(category))])), today);
  }
  ctl.setPrompt(text);
  void popover.value?.close();
  armPageMorph(sourceCard(), '/ai');
  await router.push('/ai');
};
</script>

<template>
  <button v-if="shown" ref="button" type="button" :disabled="busy" :class="['ask-ai', { lit: !direct && (direction === 'above' || direction === 'below'), open: expandedNow }]" :title="a.ask" :aria-label="a.askTitle(label)"
    aria-haspopup="dialog" :aria-expanded="expandedNow" @click="onPress">
    <Icon name="spark" :size="15" /><span>{{ a.ask }}</span>
  </button>
  <GlassPopover v-if="open && !direct" ref="popover" :anchor="button" :labelledby="titleId" @close="open = false">
    <h2 :id="titleId" class="ask-title" data-pop-item><Icon name="spark" :size="16" />{{ a.askTitle(label) }}</h2>
    <p class="ask-hint" data-pop-item>{{ workout ? a.workoutHint : night ? a.sleepHint : a.askHint }}</p>
    <button v-for="item in questions" :key="item.preset.key" type="button" class="question" data-pop-item @click="choose(item.preset, item.text)">
      <span>{{ item.text }}</span><Icon name="arrow-right" :size="15" />
    </button>
  </GlassPopover>
</template>

<style scoped>
.ask-ai { display: inline-flex; align-items: center; gap: 5px; height: 30px; padding: 0 12px 0 10px; border: 0; border-radius: 999px;
  background: var(--mat-inset); box-shadow: var(--mat-inset-shadow); color: var(--ink); font: inherit; font-size: var(--fs-xs); font-weight: 650;
  white-space: nowrap; cursor: pointer; transition: background var(--dur-base) ease, color var(--dur-base) ease, box-shadow var(--dur-base) ease, transform 160ms ease; }
.ask-ai :deep(svg) { color: var(--accent); }
.ask-ai:hover, .ask-ai.open { background: color-mix(in srgb, var(--accent) 12%, var(--mat-inset)); }
.ask-ai:active { transform: scale(.96); }
.ask-ai.lit { background: color-mix(in srgb, var(--accent) 20%, var(--mat-inset)); box-shadow: var(--mat-inset-shadow), 0 0 0 1px color-mix(in srgb, var(--accent) 45%, transparent);
  color: var(--accent); }
.ask-ai:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
.ask-ai:disabled { cursor: progress; }
.ask-title { display: flex; align-items: center; gap: 8px; margin: 0; font-size: var(--fs-md); line-height: 1.3; }
.ask-title :deep(svg) { flex: 0 0 auto; color: var(--accent); }
.ask-hint { margin: 0 0 4px; color: var(--muted); font-size: var(--fs-xs); line-height: 1.55; }
.question { display: grid; grid-template-columns: minmax(0, 1fr) 16px; align-items: center; gap: 10px; padding: 12px 14px; border: 0; border-radius: 14px;
  background: color-mix(in srgb, var(--ink) 6%, transparent); color: var(--ink); font: inherit; font-size: var(--fs-sm); line-height: 1.4; text-align: left; cursor: pointer;
  transition: background var(--dur-base) ease, transform 160ms ease; }
.question :deep(svg) { color: var(--subtle); transition: transform var(--dur-base) ease, color var(--dur-base) ease; }
.question:hover, .question:focus-visible { outline: none; background: color-mix(in srgb, var(--accent) 14%, transparent); }
.question:hover :deep(svg), .question:focus-visible :deep(svg) { color: var(--accent); transform: translateX(3px); }
.question:active { transform: scale(.985); }
</style>
