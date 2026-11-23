<script setup lang="ts">
import { computed, ref } from 'vue';
import { useGlassLens } from '../composables/useGlassLens';

const props = withDefaults(defineProps<{
  modelValue: boolean;
  disabled?: boolean;
  ariaLabel?: string;
  ariaLabelledby?: string;
  title?: string;
}>(), { disabled: false });
const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>();

const TRAVEL = 18;
const DRAG_SLOP = 3;

const lensEl = ref<HTMLElement | null>(null);
const lens = useGlassLens(lensEl, 'thumb');
const lifted = ref(false);
const dragging = ref(false);
const dragPos = ref<number | null>(null);
const pos = computed(() => dragPos.value ?? (props.modelValue ? TRAVEL : 0));

let gesture: { id: number; x: number; start: number } | null = null;
let suppressClick = false;

const down = (event: PointerEvent) => {
  if (props.disabled || event.button !== 0) return;
  gesture = { id: event.pointerId, x: event.clientX, start: props.modelValue ? TRAVEL : 0 };
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  lifted.value = true;
};
const move = (event: PointerEvent) => {
  if (!gesture || gesture.id !== event.pointerId) return;
  const dx = event.clientX - gesture.x;
  if (!dragging.value && Math.abs(dx) < DRAG_SLOP) return;
  dragging.value = true;
  dragPos.value = Math.min(TRAVEL, Math.max(0, gesture.start + dx));
};
const finish = (event: PointerEvent, cancelled: boolean) => {
  if (!gesture || gesture.id !== event.pointerId) return;
  gesture = null;
  lifted.value = false;
  if (dragging.value) {
    const next = (dragPos.value ?? 0) > TRAVEL / 2;
    suppressClick = true;
    if (!cancelled && next !== props.modelValue) emit('update:modelValue', next);
  }
  dragging.value = false;
  dragPos.value = null;
};
const click = () => {
  if (suppressClick) { suppressClick = false; return; }
  if (!props.disabled) emit('update:modelValue', !props.modelValue);
};
</script>

<template>
  <button
    type="button"
    role="switch"
    :class="['glass-switch', { on: modelValue, lifted, dragging, 'has-lens': lens.active.value }]"
    :style="{ '--gs-x': `${pos}px`, '--gs-p': pos / TRAVEL }"
    :aria-checked="modelValue"
    :aria-label="ariaLabel"
    :aria-labelledby="ariaLabelledby"
    :title="title"
    :disabled="disabled"
    @pointerenter="lens.prime()"
    @pointerdown="down"
    @pointermove="move"
    @pointerup="finish($event, false)"
    @pointercancel="finish($event, true)"
    @lostpointercapture="finish($event, true)"
    @click.stop="click"
  >
    <span class="gs-fill" aria-hidden="true"></span>
    <span class="gs-knob" aria-hidden="true">
      <span class="gs-disc"></span>
      <span ref="lensEl" class="gs-lens" :style="lens.style()"></span>
      <span class="gs-rim"></span>
    </span>
  </button>
</template>

<style scoped>
.glass-switch {
  position: relative;
  flex: 0 0 44px;
  width: 44px;
  height: 26px;
  padding: 0;
  border: 0;
  border-radius: 999px;
  background: var(--mat-inset);
  box-shadow: var(--mat-inset-shadow);
  cursor: pointer;
  touch-action: pan-y;
  -webkit-tap-highlight-color: transparent;
}
.glass-switch:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
.glass-switch:disabled { opacity: .5; cursor: not-allowed; }
.gs-fill {
  position: absolute;
  inset: 0;
  border-radius: inherit;
  background: var(--accent);
  box-shadow: inset 0 1px 2px rgba(0, 0, 0, .25);
  opacity: var(--gs-p);
  transition: opacity var(--dur-base) ease;
}
.dragging .gs-fill { transition: none; }
.gs-knob {
  position: absolute;
  top: 13px;
  left: 13px;
  width: 0;
  height: 0;
  translate: var(--gs-x) 0;
  transition: translate 340ms cubic-bezier(.3, 1.35, .5, 1);
}
.dragging .gs-knob { transition: none; }
.gs-disc, .gs-lens, .gs-rim { position: absolute; border-radius: 999px; pointer-events: none; }
.gs-disc {
  top: -10px;
  left: -10px;
  width: 20px;
  height: 20px;
  background: linear-gradient(180deg, #FFFFFF, #DCE1E6);
  box-shadow: 0 1px 3px rgba(0, 0, 0, .35);
  transition: scale 220ms cubic-bezier(.2, .8, .2, 1), opacity 180ms ease;
}
.gs-lens, .gs-rim {
  top: -14px;
  left: -19px;
  width: 38px;
  height: 28px;
  opacity: 0;
  scale: .58;
  visibility: hidden;
  transition: scale 260ms cubic-bezier(.3, 1.3, .5, 1), opacity 160ms ease, visibility 0s linear 260ms;
}
.gs-rim { box-shadow: var(--lens-glass-rim); }
.gs-rim::before {
  content: '';
  position: absolute;
  inset: 0;
  padding: 1.2px;
  border-radius: inherit;
  background: var(--lens-glass-specular);
  -webkit-mask: linear-gradient(#000 0 0) content-box exclude, linear-gradient(#000 0 0);
  mask: linear-gradient(#000 0 0) content-box exclude, linear-gradient(#000 0 0);
}
.glass-switch:not(.has-lens) .gs-lens { background: color-mix(in srgb, #fff 26%, transparent); }
.lifted .gs-disc { scale: 1.5; opacity: 0; }
.lifted .gs-lens, .lifted .gs-rim { scale: 1; opacity: 1; visibility: visible; transition-delay: 0s; }
@media (prefers-reduced-motion: reduce) {
  .gs-fill, .gs-knob, .gs-disc, .gs-lens, .gs-rim { transition: none; }
}
</style>
