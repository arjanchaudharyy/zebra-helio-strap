<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import CapsuleWheel from '../../CapsuleWheel.vue';
import SegmentTrack from '../../SegmentTrack.vue';
import Icon from '../../Icon.vue';
import ReceiptFace from './ReceiptFace.vue';
import PlanFace from './PlanFace.vue';
import { reducedMotion } from '../../../lib/motion/cards';
import { paced } from '../../../lib/motion/timing';
import type { AiTaskIssue, AiTaskPrepareResult, AiTaskTemplate } from '../../../lib/bridge/types';
import type { DayRow } from '../../../lib/trainingPlan/week';
import type { TrainingPlanState } from '../../../types/trainingPlan';
import type { HandoffStep, HandoffStepId } from '../../../composables/useAiTaskHandoff';
import type { FutureFace } from '../../../composables/ai/useStageSend';
import { useAiTaskDraft } from '../../../composables/useAiTaskDraft';
import { aiTaskIssueText } from '../../../lib/aiTask/copy';
import type { Box } from '../../../lib/aiTask/stageLayout';
import { useBridgeText } from '../bridge/bridge.i18n';
import { useStageText } from './stage.i18n';

const props = defineProps<{
  box: Box; face: FutureFace; templates: AiTaskTemplate[]; disabled: boolean;
  ready: AiTaskPrepareResult | null; blocked: AiTaskIssue[]; provider: string; providerIcon: string; stale: boolean; recalled: boolean;
  steps: Record<HandoffStepId, HandoffStep>; saveError: string | null;
  rows: DayRow[]; drafting: boolean; plan: TrainingPlanState | null; mcp: boolean; busy: boolean; simulated: boolean;
  detach?: () => Promise<void>;
}>();
const emit = defineEmits<{
  'update:face': [FutureFace]; copy: []; open: []; reveal: []; retry: [HandoffStepId]; received: []; accept: []; discard: []; workout: []; undo: []; clear: [];
}>();
const ctl = useAiTaskDraft();
const t = useBridgeText();
const s = useStageText();
const front = ref<HTMLElement | null>(null);
const input = ref<HTMLTextAreaElement | null>(null);
const body = ref<HTMLElement | null>(null);

type Intent = 'free' | 'sleep' | 'week' | 'workout' | 'next';
const TEMPLATE_OF: Record<Exclude<Intent, 'free'>, { id: string; days: number }> = {
  sleep: { id: 'sleep_review', days: 14 }, week: { id: 'week_review', days: 7 }, workout: { id: 'recovery_run', days: 14 }, next: { id: 'next_week', days: 30 },
};
const intents = computed(() => [
  { value: 'free' as Intent, label: s.value.freeIntent },
  { value: 'sleep' as Intent, label: s.value.sleepShort, title: t.value.sleepIntentHint },
  { value: 'week' as Intent, label: s.value.weekShort, title: t.value.weekIntentHint },
  { value: 'workout' as Intent, label: s.value.workoutShort, title: t.value.workoutIntentHint },
  { value: 'next' as Intent, label: s.value.nextShort, title: t.value.nextIntentHint },
]);
const intent = computed<Intent>(() => (Object.entries(TEMPLATE_OF).find(([, v]) => v.id === ctl.draft.value.template_id)?.[0] as Intent | undefined) ?? 'free');
const questionsOf = (text: typeof t.value) => ({ sleep: text.sleepQuestion, week: text.weekQuestion, workout: text.workoutQuestion, next: text.nextQuestion });
const pick = async (next: Intent) => {
  if (props.disabled || next === intent.value) return;
  if (next === 'free') {
    ctl.setTemplate(null);
  } else {
    const spec = TEMPLATE_OF[next];
    ctl.setTemplate(props.templates.find((p) => p.id === spec.id) ?? null);
    ctl.setWindowDays(spec.days - 1);
    ctl.setPrompt(questionsOf(t.value)[next]);
    if (next === 'workout') emit('workout');
  }
  if (props.face !== 'question') await turn('question');
  await nextTick();
  input.value?.focus();
};

watch(t, (now, before) => {
  if (!before || now === before) return;
  const prompt = ctl.draft.value.prompt.trim();
  const was = questionsOf(before);
  const key = (Object.keys(was) as Array<keyof typeof was>).find((k) => was[k].trim() === prompt);
  if (key && questionsOf(now)[key] !== ctl.draft.value.prompt) ctl.setPrompt(questionsOf(now)[key]);
});

const wheelItems = computed(() => intents.value.map(({ value, label }) => ({ value, label })));
const wheelSpan = computed(() => Math.max(160, Math.round(props.box.width - 36)));
const intentHint = computed(() => intents.value.find((one) => one.value === intent.value)?.title ?? null);

const EASE = 'cubic-bezier(.4, .6, .2, 1)';
const side = computed(() => (props.face === 'plan' ? 'plan' : 'question'));
const sides = computed(() => [{ value: 'question', label: s.value.faceAsk }, { value: 'plan', label: s.value.facePlan }]);
const askFace = ref<'question' | 'receipt'>(props.face === 'receipt' ? 'receipt' : 'question');
watch(() => props.face, (face) => { if (face !== 'plan') askFace.value = face; });
const pickSide = (value: string) => { void turn(value === 'plan' ? 'plan' : askFace.value); };

const crossfade = async (next: FutureFace) => {
  const el = body.value!;
  const out = el.animate([{ opacity: 1 }, { opacity: 0 }], { duration: paced(140), easing: 'cubic-bezier(.4, 0, 1, 1)', fill: 'forwards' });
  await out.finished.catch(() => undefined);
  pin();
  emit('update:face', next);
  await nextTick();
  const fadeIn = el.animate([{ opacity: 0, transform: 'translateY(4px)' }, { opacity: 1, transform: 'none' }], { duration: paced(300), delay: paced(160), easing: EASE, fill: 'backwards' });
  out.cancel();
  await fadeIn.finished.catch(() => undefined);
  unpin();
};
let turning: Promise<void> | null = null;
const turn = async (next: FutureFace) => {
  if (turning) await turning;
  if (next === props.face) return;
  const detaching = next === 'question' && props.face === 'receipt' && props.detach;
  if (!detaching && (!front.value || !body.value || reducedMotion())) { emit('update:face', next); return; }
  turning = detaching ? props.detach!() : crossfade(next);
  await turning;
  turning = null;
};

const chrome = ref(76);
const bodyHeight = computed(() => (props.face === 'plan' ? undefined : `${Math.max(180, props.box.height - chrome.value)}px`));
let observer: ResizeObserver | null = null;
const pin = () => { if (front.value) front.value.style.height = `${front.value.offsetHeight}px`; };
const unpin = () => { if (front.value) front.value.style.height = ''; };
let shown = 0;
let grow: Animation | null = null;
onMounted(() => {
  const card = front.value;
  const inner = body.value;
  if (!card || !inner) return;
  chrome.value = card.offsetHeight - inner.offsetHeight;
  shown = card.offsetHeight;
  observer = new ResizeObserver(() => {
    const next = inner.offsetHeight + chrome.value;
    if (Math.abs(next - shown) < 1) return;
    const from = grow?.playState === 'running' ? parseFloat(getComputedStyle(card).height) : shown;
    grow?.cancel();
    shown = next;
    unpin();
    if (reducedMotion()) return;
    grow = card.animate([{ height: `${from}px` }, { height: `${next}px` }], { duration: paced(400), easing: EASE });
  });
  observer.observe(inner);
});
onBeforeUnmount(() => { observer?.disconnect(); grow?.cancel(); });

defineExpose({ front, body, turn });
</script>

<template>
  <div class="future" :style="{ left: `${box.x}px`, top: `${box.y}px`, width: `${box.width}px`, height: `${box.height}px` }">
    <section ref="front" :class="['fcard', `face-${face}`]">
      <SegmentTrack class="face-switch" variant="glass" compact :items="sides" :model-value="side" :disabled="disabled" :aria-label="t.future" @update:model-value="pickSide" />
      <div ref="body" :class="['face-body', `body-${face}`]" :style="{ height: bodyHeight }">
        <template v-if="face === 'question'">
          <label class="question">
            <span>{{ t.composer }}</span>
            <textarea ref="input" :value="ctl.draft.value.prompt" :placeholder="t.placeholder" :disabled="disabled" @input="ctl.setPrompt(($event.target as HTMLTextAreaElement).value)"></textarea>
          </label>
          <div class="templates">
            <CapsuleWheel class="intent-wheel" loop :span="wheelSpan" :items="wheelItems" :model-value="intent" :disabled="disabled" :aria-label="t.composer" @update:model-value="pick" />
            <Transition name="hint-swap" mode="out-in"><small v-if="intentHint" :key="intent" class="intent-hint">{{ intentHint }}</small></Transition>
          </div>
          <ul v-if="blocked.length || (steps.prepare.state === 'failed' && steps.prepare.errorText)" class="issues" role="alert">
            <li v-for="(issue, i) in blocked" :key="i"><Icon name="warning" :size="13" />{{ aiTaskIssueText(issue) }}</li>
            <li v-if="steps.prepare.state === 'failed' && steps.prepare.errorText"><Icon name="warning" :size="13" />{{ steps.prepare.errorText }}</li>
          </ul>
        </template>
        <ReceiptFace v-else-if="face === 'receipt'" :ready="ready" :provider="provider" :provider-icon="providerIcon" :stale="stale" :recalled="recalled" :steps="steps" :save-error="saveError"
          :question="ctl.draft.value.prompt" @copy="emit('copy')" @open="emit('open')" @reveal="emit('reveal')" @retry="emit('retry', $event)"
          @edit="turn('question')" @received="emit('received')" />
        <PlanFace v-else :rows="rows" :drafting="drafting" :state="plan" :mcp="mcp" :busy="busy" :simulated="simulated"
          @received="emit('received')" @accept="emit('accept')" @discard="emit('discard')" @undo="emit('undo')" @clear="emit('clear')" />
      </div>
    </section>
  </div>
</template>

<style scoped>
.future { position: absolute; }
.fcard { position: absolute; top: 50%; right: 0; left: 0; display: flex; flex-direction: column; gap: 12px; padding: 12px 18px 16px; overflow: hidden;
  border: 1px solid var(--mat-line); border-radius: 24px; background: var(--mat-card); box-shadow: var(--mat-rim), var(--mat-shadow); color: var(--ink); translate: 0 -50%; }
.face-switch { align-self: center; }
.face-body { flex-shrink: 0; min-height: 0; }
.body-question { display: grid; grid-template-rows: minmax(0, 1fr) auto auto; gap: 12px; }
.body-receipt { display: grid; }
.templates { display: grid; justify-items: center; gap: 6px; min-width: 0; padding-top: 12px; border-top: 1px solid var(--mat-line); }
.intent-wheel { max-width: 100%; }
.intent-wheel :deep(.wheel-item) { font-size: var(--fs-sm); }
.intent-hint { max-width: 100%; color: var(--subtle); font-size: var(--fs-2xs); line-height: 1.5; text-align: center; }
.hint-swap-enter-active, .hint-swap-leave-active { transition: opacity 160ms ease, translate 200ms ease; }
.hint-swap-enter-from { opacity: 0; translate: 0 4px; }
.hint-swap-leave-to { opacity: 0; translate: 0 -4px; }
.question { display: grid; grid-template-rows: auto 1fr; gap: 10px; min-height: 0; }
.question > span { color: var(--subtle); font-size: var(--fs-xs); }
.question textarea { width: 100%; min-height: 0; padding: 0; border: 0; outline: 0; background: none; resize: none; color: var(--ink); font: 17px / 1.65 var(--font-sans); }
.question textarea::placeholder { color: var(--subtle); opacity: .75; }
.fcard.face-question:focus-within { box-shadow: var(--mat-rim), var(--mat-shadow), inset 0 0 0 1px var(--line-strong); }
.issues { display: grid; gap: 6px; margin: 10px 0 0; padding: 0; list-style: none; color: var(--danger, var(--warning)); font-size: var(--fs-2xs); }
.issues li { display: flex; gap: 6px; }
</style>
