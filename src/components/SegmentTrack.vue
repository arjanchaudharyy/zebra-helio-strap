<script setup lang="ts" generic="T extends string | number">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue';
import { fontsReady } from '../lib/fontsReady';
import Icon, { type IconName } from './Icon.vue';
import { dragThumb, FLICK_VELOCITY, snapStop, type SegmentStop } from '../lib/navigation';
import { flickDeform, liftRect, rubberStretch, stretchLimit } from '../lib/segmentGlass';
import { poseStyle, springFrames, stretchPose } from '../lib/glassGroup';
import { useGlassMember } from '../composables/useGlassGroup';
import { useGlassLens } from '../composables/useGlassLens';
import { exemptFromSettle } from '../lib/motion/interrupt';
import { lessMotion } from '../lib/motion/lessMotion';

export type SegmentItem<T extends string | number> = { value: T; label: string; icon?: IconName; title?: string; short?: string };

const props = withDefaults(defineProps<{
  items: SegmentItem<T>[];
  modelValue: T;
  ariaLabel?: string;
  disabled?: boolean;
  compact?: boolean;
  fill?: boolean;
  iconOnly?: boolean;
  variant?: 'inset' | 'glass' | 'bare';
  intro?: boolean;
  noWrap?: boolean;
  placeholder?: T;
}>(), {
  placeholder: undefined,
  intro: false,
  noWrap: false,
  disabled: false,
  compact: false,
  fill: false,
  iconOnly: false,
  variant: 'inset',
});

const emit = defineEmits<{
  'update:modelValue': [value: T];
  reselect: [value: T];
}>();

const track = ref<HTMLElement | null>(null);
const stops = ref([]) as Ref<SegmentStop<T>[]>;
const thumb = ref({ left: 0, width: 0, top: 0, height: 0, visible: false });
const fit = ref(0);
const FIT_WRAP = 3;
const wrapped = computed(() => fit.value >= FIT_WRAP);
const compactNow = computed(() => props.compact || fit.value >= 1);
const shortNow = computed(() => fit.value >= 2);
const shown = (item: SegmentItem<T>) => (shortNow.value && item.short ? item.short : item.label);
const fullTitle = (item: SegmentItem<T>) => item.title ?? (shortNow.value && item.short && item.short !== item.label ? item.label : undefined);
const dragging = ref(false);
const settling = ref(false);
const lensValue = ref<T | null>(null) as Ref<T | null>;
const lensFade = ref(false);
let fadeFrame = 0;

type Gesture = {
  id: number;
  x: number;
  center: number;
  startValue: T;
  lastX: number;
  time: number;
  velocity: number;
  row: number | null;
};
let gesture: Gesture | null = null;
let frame = 0;
let nextThumb: { left: number; width: number } | null = null;
let suppressClick = false;
let observer: ResizeObserver | null = null;
let staleStops = false;

const lensEl = ref<HTMLElement | null>(null);
const glassEl = ref<HTMLElement | null>(null);
const thumbEl = ref<HTMLElement | null>(null);
const plateEl = ref<HTMLElement | null>(null);
const rimEl = ref<HTMLElement | null>(null);
const rimShape = ref<HTMLElement | null>(null);
const thumbLens = useGlassLens(lensEl, 'thumb');
const rimLens = useGlassLens(glassEl, 'rim', props.variant === 'glass');
const pressed = ref(false);
const flyLift = ref(false);
const trackSize = ref({ w: 0, h: 0 });
const trackPad = ref(3);
const stretch = ref(0);
const FLY_MS = 360;
const FLY_EASE = 'cubic-bezier(.3, 1.22, .4, 1)';
const HOP_MS = 380;
const HOP_SHRINK = 0.6;
let flyToken = 0;
let colorTimer = 0;
const LIFT_MS = 380;
const DROP_MS = 260;
const LIFT_EASE = 'cubic-bezier(.3, 1.45, .45, 1)';
const DROP_EASE = 'cubic-bezier(.25, .8, .3, 1)';

const reducedMotion = () => lessMotion();

const buttons = () => Array.from(track.value?.querySelectorAll<HTMLElement>('.segment-item') ?? []);
const readStops = (): SegmentStop<T>[] => buttons().map((el, index) => ({
  left: el.offsetLeft,
  width: el.offsetWidth,
  top: el.offsetTop,
  height: el.offsetHeight,
  value: props.items[index]!.value,
}));

const parked = computed(() => props.placeholder !== undefined && !props.items.some((item) => item.value === props.modelValue));
const placeOn = (value: T) => {
  const stop = stops.value.find((item) => item.value === value)
    ?? (parked.value && value === props.modelValue ? stops.value.find((item) => item.value === props.placeholder) : undefined);
  thumb.value = stop
    ? { left: stop.left, width: stop.width, top: stop.top ?? 0, height: stop.height ?? 0, visible: true }
    : { ...thumb.value, visible: false };
};
const restsOn = (value: T) => {
  const stop = stops.value.find((item) => item.value === value);
  return !!stop && stop.left === thumb.value.left && stop.top === thumb.value.top && thumb.value.visible;
};

const ladderOn = () => !props.noWrap && props.variant === 'inset';
const overflows = (el: HTMLElement) => {
  const pad = Number.parseFloat(getComputedStyle(el).paddingLeft) || 0;
  const natural = buttons().reduce((sum, button) => sum + button.offsetWidth, 0) + pad * 2;
  return natural > el.clientWidth + 1;
};
let fitting = false;
let refitAgain = false;
const refit = async () => {
  const el = track.value;
  if (!el || !ladderOn()) { fit.value = 0; return; }
  if (fitting) { refitAgain = true; return; }
  fitting = true;
  try {
    fit.value = 0;
    await nextTick();
    while (fit.value < FIT_WRAP && overflows(el)) {
      fit.value += 1;
      if (fit.value === 1 && props.compact) continue;
      if (fit.value === 2 && !props.items.some((item) => item.short && item.short !== item.label)) continue;
      await nextTick();
    }
  } finally {
    fitting = false;
  }
  if (refitAgain) { refitAgain = false; await refit(); }
};

const measure = () => {
  if (!track.value) return;
  trackSize.value = { w: track.value.offsetWidth, h: track.value.offsetHeight };
  trackPad.value = Number.parseFloat(getComputedStyle(track.value).paddingTop) || 0;
  stops.value = readStops();
  if (!gesture && !settling.value) placeOn(props.modelValue);
  else if (settling.value) staleStops = true;
};

const fitAndMeasure = async () => {
  if (!gesture) await refit();
  measure();
};

const observeItems = () => {
  if (!observer || !track.value) return;
  observer.disconnect();
  observer.observe(track.value);
  if (track.value.parentElement) observer.observe(track.value.parentElement);
  for (const el of buttons()) observer.observe(el);
};

const lensRect = computed(() => liftRect(thumb.value, trackSize.value.w, trackPad.value));

const trackStyle = computed(() => {
  const lens = lensRect.value;
  const style: Record<string, string | number> = {
    '--thumb-l': `${thumb.value.left}px`,
    '--thumb-w': `${thumb.value.visible ? thumb.value.width : 0}px`,
    '--thumb-t': `${thumb.value.top}px`,
    '--thumb-h': `${thumb.value.height}px`,
    '--thumb-vis': thumb.value.visible ? 1 : 0,
    '--lens-x': `${lens.x.toFixed(2)}px`,
    '--lens-y': `${lens.y.toFixed(2)}px`,
    '--lens-w': `${lens.w}px`,
    '--lens-h': `${lens.h}px`,
  };
  return { ...style, ...poseStyle(pose.value) };
});
const lifted = computed(() => thumbLens.active.value && (pressed.value || dragging.value || flyLift.value));
const pose = computed(() => stretchPose(stretch.value, trackSize.value.w, lifted.value));
useGlassMember('segment', () => track.value, () => pose.value);
const offRow = (index: number) => wrapped.value && (stops.value[index]?.top ?? 0) !== thumb.value.top;
const itemLeft = (index: number) => ({ '--item-l': `${stops.value[index]?.left ?? 0}px` });

const focusActive = () => {
  if (!track.value?.contains(document.activeElement)) return;
  const index = props.items.findIndex((item) => item.value === props.modelValue);
  buttons()[index]?.focus({ preventScroll: true });
};

const commit = (value: T) => {
  if (value !== props.modelValue) emit('update:modelValue', value);
};

const layoutScale = () => {
  if (!track.value || !track.value.offsetWidth) return 1;
  const pulled = 1 + Math.abs(stretch.value) / track.value.offsetWidth;
  return track.value.getBoundingClientRect().width / track.value.offsetWidth / pulled || 1;
};

const flying: Animation[] = [];
const fly = async (value: T, lift: boolean, fromColor: T | null = null) => {
  const token = (flyToken += 1);
  const movers = [thumbEl.value, rimEl.value, lensEl.value];
  const before = movers.map((el) => el?.getBoundingClientRect() ?? null);
  const wasVisible = thumb.value.visible;
  const targetTop = stops.value.find((stop) => stop.value === value)?.top ?? 0;
  const hop = wrapped.value && wasVisible && targetTop !== thumb.value.top;
  if (hop) lift = false;
  if (lift && thumbLens.active.value) flyLift.value = true;
  placeOn(value);
  settling.value = true;
  window.clearTimeout(colorTimer);
  const startColor = fromColor ?? props.modelValue;
  const quiet = !wasVisible || reducedMotion();
  if (quiet || startColor === value) {
    lensValue.value = value;
  } else {
    lensValue.value = startColor;
    lensFade.value = false;
    cancelAnimationFrame(fadeFrame);
    fadeFrame = requestAnimationFrame(() => { fadeFrame = requestAnimationFrame(() => { lensFade.value = true; }); });
    colorTimer = window.setTimeout(() => { if (token === flyToken) lensValue.value = value; }, hop ? HOP_MS * 0.5 : FLY_MS * 0.3);
  }
  const land = () => {
    if (token !== flyToken) return;
    settling.value = false;
    if (staleStops && !gesture) { staleStops = false; stops.value = readStops(); placeOn(props.modelValue); }
    flyLift.value = false;
    lensFade.value = false;
    lensValue.value = null;
  };
  await nextTick();
  if (quiet) {
    land();
    return;
  }
  for (const animation of flying.splice(0)) animation.cancel();
  const scale = layoutScale();
  if (hop) {
    const el = thumbEl.value;
    const from = before[0];
    const to = el?.getBoundingClientRect();
    if (el && from?.width && to?.width) {
      const dx = (from.left + from.width / 2 - (to.left + to.width / 2)) / scale;
      const dy = (from.top + from.height / 2 - (to.top + to.height / 2)) / scale;
      const sw = from.width / to.width;
      const sh = from.height / to.height;
      const at = `${dx}px ${dy}px`;
      flying.push(exemptFromSettle(el.animate([
        { translate: at, scale: `${sw} ${sh}`, opacity: 1, easing: 'cubic-bezier(.4, 0, .7, .4)' },
        { translate: at, scale: `${sw * HOP_SHRINK} ${sh * HOP_SHRINK}`, opacity: 0, offset: 0.4 },
        { translate: '0px 0px', scale: `${HOP_SHRINK} ${HOP_SHRINK}`, opacity: 0, offset: 0.44, easing: 'cubic-bezier(.3, .6, .3, 1.15)' },
        { translate: '0px 0px', scale: '1 1', opacity: 1 },
      ], { duration: HOP_MS })));
    }
  }
  if (!hop) movers.forEach((el, index) => {
    const from = before[index];
    const to = el?.getBoundingClientRect();
    if (!el || !from || !from.width || !to || !to.width) return;
    const dx = (from.left + from.width / 2 - (to.left + to.width / 2)) / scale;
    const dy = (from.top + from.height / 2 - (to.top + to.height / 2)) / scale;
    const keyframes: Keyframe[] = el === lensEl.value
      ? [{ translate: `${dx}px ${dy}px` }, { translate: '0px 0px' }]
      : [{ translate: `${dx}px ${dy}px`, scale: `${from.width / to.width} ${from.height / to.height}` }, { translate: '0px 0px', scale: '1 1' }];
    flying.push(exemptFromSettle(el.animate(keyframes, { duration: FLY_MS, easing: FLY_EASE })));
  });
  if (flying.length) void Promise.all(flying.map((animation) => animation.finished)).then(land, () => undefined);
  else land();
  await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
};

const morphing: Animation[] = [];
type LayerPose = { scale: string; translate: string; opacity: string };
const poseOf = (el: HTMLElement): LayerPose => {
  const style = getComputedStyle(el);
  return {
    scale: style.scale === 'none' ? '1 1' : style.scale,
    translate: style.translate === 'none' ? '0px 0px' : style.translate,
    opacity: style.opacity,
  };
};
watch(lifted, async (up) => {
  const midway = morphing.some((animation) => animation.playState === 'running');
  const poses = new Map<HTMLElement, LayerPose>();
  if (midway) {
    for (const el of [plateEl.value, rimShape.value, lensEl.value]) if (el) poses.set(el, poseOf(el));
  }
  for (const animation of morphing.splice(0)) animation.cancel();
  if (reducedMotion()) return;
  await nextTick();
  const plate = plateEl.value;
  const rim = rimShape.value;
  const lens = lensEl.value;
  const box = thumb.value;
  const glass = lensRect.value;
  if (!plate || !rim || !lens || !box.width || !glass.w || lifted.value !== up) return;
  const sx = box.width / glass.w;
  const sy = box.height / glass.h;
  const dx = box.left + box.width / 2 - (glass.x + glass.w / 2);
  const dy = box.top + box.height / 2 - (glass.y + glass.h / 2);
  const rest = { scale: `${sx} ${sy}`, translate: `${dx}px ${dy}px` };
  const grown = { scale: `${1 / sx} ${1 / sy}`, translate: `${-dx}px ${-dy}px` };
  const flat = { scale: '1 1', translate: '0px 0px' };
  const timing: KeyframeAnimationOptions = { duration: up ? LIFT_MS : DROP_MS, easing: up ? LIFT_EASE : DROP_EASE };
  const fade = (duration: number, delay: number): KeyframeAnimationOptions => ({ duration, delay, easing: 'ease', fill: 'backwards' });
  const parts: Array<[HTMLElement, Keyframe[], KeyframeAnimationOptions]> = up
    ? [
      [rim, [rest, flat], timing],
      [plate, [flat, grown], timing],
      [rim, [{ opacity: 0 }, { opacity: 1 }], fade(120, 0)],
      [plate, [{ opacity: 1 }, { opacity: 0 }], fade(200, 60)],
      [lens, [{ opacity: 0 }, { opacity: 1 }], fade(180, 60)],
    ]
    : [
      [rim, [flat, rest], timing],
      [plate, [grown, flat], timing],
      [lens, [{ opacity: 1 }, { opacity: 0 }], fade(140, 0)],
      [plate, [{ opacity: 0 }, { opacity: 1 }], fade(180, 40)],
      [rim, [{ opacity: 1 }, { opacity: 0 }], fade(120, Math.max(0, DROP_MS - 120))],
    ];
  const from = (el: HTMLElement, frame: Keyframe): Keyframe => {
    const pose = poses.get(el);
    return pose ? Object.fromEntries(Object.keys(frame).map((key) => [key, pose[key as keyof LayerPose]])) : frame;
  };
  for (const [el, [first, ...after], options] of parts) {
    morphing.push(exemptFromSettle(el.animate([from(el, first!), ...after], options)));
  }
}, { flush: 'pre' });

let elastic: Animation | null = null;
const springBack = (from: number, flick: number) => {
  const el = track.value;
  const spring = springFrames(from, flick, trackSize.value.w);
  if (!el || !spring || reducedMotion()) return;
  elastic?.cancel();
  elastic = exemptFromSettle(el.animate(spring.frames, { duration: spring.duration }));
};

const clearGesture = () => {
  const current = gesture;
  gesture = null;
  dragging.value = false;
  pressed.value = false;
  lensValue.value = null;
  stretch.value = 0;
  cancelAnimationFrame(fadeFrame);
  cancelAnimationFrame(frame);
  frame = 0;
  nextThumb = null;
  if (current && track.value?.hasPointerCapture(current.id)) track.value.releasePointerCapture(current.id);
};

const primeLens = () => { thumbLens.prime(); rimLens.prime(); };
const onDown = (event: PointerEvent) => {
  if (props.disabled || event.button !== 0 || !event.isPrimary || !track.value) return;
  primeLens();
  measure();
  elastic?.cancel();
  elastic = null;
  const button = (event.target as Element).closest<HTMLElement>('.segment-item');
  const index = button ? buttons().indexOf(button) : -1;
  const startValue = index >= 0 ? props.items[index]!.value : props.modelValue;
  const active = stops.value.find((stop) => stop.value === props.modelValue) ?? stops.value[0];
  if (!active) return;
  gesture = {
    id: event.pointerId,
    x: event.clientX,
    center: active.left + active.width / 2,
    startValue,
    lastX: event.clientX,
    time: event.timeStamp,
    velocity: 0,
    row: wrapped.value ? active.top ?? 0 : null,
  };
  track.value.setPointerCapture(event.pointerId);
  if (startValue === props.modelValue) pressed.value = true;
};

const rowAt = (clientY: number): number => {
  const rect = track.value!.getBoundingClientRect();
  const y = (clientY - rect.top) / layoutScale();
  let best = stops.value[0]?.top ?? 0;
  for (const stop of stops.value) {
    const top = stop.top ?? 0;
    if (Math.abs(top + (stop.height ?? 0) / 2 - y) < Math.abs(best + (stop.height ?? 0) / 2 - y)) best = top;
  }
  return best;
};
const stopsIn = (row: number | null) => (row === null ? stops.value : stops.value.filter((stop) => (stop.top ?? 0) === row));
const ROW_SLIDE_MS = 200;
const slideRow = async (apply: () => void) => {
  const movers = [thumbEl.value, rimEl.value, lensEl.value];
  const before = movers.map((el) => el?.getBoundingClientRect() ?? null);
  apply();
  await nextTick();
  if (reducedMotion()) return;
  const scale = layoutScale();
  movers.forEach((el, index) => {
    const from = before[index];
    const to = el?.getBoundingClientRect();
    if (!el || !from || !to || !to.height) return;
    const dy = (from.top - to.top) / scale;
    if (Math.abs(dy) < 1) return;
    exemptFromSettle(el.animate([{ translate: `0px ${dy}px` }, { translate: '0px 0px' }], { duration: ROW_SLIDE_MS, easing: DROP_EASE }));
  });
};

const overshoot = (center: number) => {
  const first = stops.value[0];
  const last = stops.value[stops.value.length - 1];
  if (!first || !last) return 0;
  const lo = first.left + first.width / 2;
  const hi = last.left + last.width / 2;
  return center < lo ? center - lo : center > hi ? center - hi : 0;
};

const onMove = (event: PointerEvent) => {
  const current = gesture;
  if (!current || current.id !== event.pointerId) return;
  const scale = layoutScale();
  const dx = (event.clientX - current.x) / scale;
  if (!dragging.value && Math.abs(dx) < 5) return;
  if (!dragging.value) {
    for (const animation of flying.splice(0)) animation.cancel();
    flyToken += 1;
    window.clearTimeout(colorTimer);
    settling.value = false;
    flyLift.value = false;
    lensValue.value = props.modelValue;
    lensFade.value = false;
    fadeFrame = requestAnimationFrame(() => { fadeFrame = requestAnimationFrame(() => { lensFade.value = true; }); });
  }
  dragging.value = true;
  suppressClick = true;
  const instant = (event.clientX - current.lastX) / scale / Math.max(1, event.timeStamp - current.time);
  current.velocity += (instant - current.velocity) * 0.35;
  current.lastX = event.clientX;
  current.time = event.timeStamp;
  const center = current.center + dx;
  if (current.row !== null) current.row = rowAt(event.clientY);
  const pool = stopsIn(current.row);
  nextThumb = dragThumb(pool, center, current.velocity);
  const pull = current.row === null ? rubberStretch(overshoot(center), stretchLimit(trackSize.value.w)) : 0;
  if (!frame) {
    frame = requestAnimationFrame(() => {
      frame = 0;
      const live = gesture;
      if (!nextThumb || !live) return;
      const rowStops = stopsIn(live.row);
      const row = rowStops[0];
      const apply = () => {
        thumb.value = { ...thumb.value, ...nextThumb!, top: row?.top ?? thumb.value.top, height: row?.height ?? thumb.value.height, visible: true };
      };
      if (row && (row.top ?? 0) !== thumb.value.top) void slideRow(apply);
      else apply();
      stretch.value = pull;
      lensValue.value = snapStop(rowStops, nextThumb.left + nextThumb.width / 2, 0).value;
    });
  }
  event.preventDefault();
};

const onUp = (event: PointerEvent) => {
  const current = gesture;
  if (!current || current.id !== event.pointerId) return;
  const pulled = stretch.value;
  if (event.type !== 'pointerup') {
    const landing = dragging.value ? snapStop(stopsIn(current.row), thumb.value.left + thumb.value.width / 2, 0).value : null;
    clearGesture();
    springBack(pulled, 0);
    window.setTimeout(() => { suppressClick = false; }, 0);
    if (landing !== null) {
      void fly(landing, false);
      commit(landing);
    } else {
      placeOn(props.modelValue);
    }
    return;
  }
  const moved = dragging.value;
  const center = current.center + (event.clientX - current.x) / layoutScale();
  const velocity = event.timeStamp - current.time < 90 ? current.velocity : 0;
  const next = moved ? snapStop(stopsIn(current.row), center, velocity).value : current.startValue;
  const keepFade = lensFade.value;
  const colored = lensValue.value;
  const willFly = moved || next !== props.modelValue;
  if (willFly && thumbLens.active.value) flyLift.value = true;
  clearGesture();
  springBack(pulled, moved && Math.abs(velocity) >= FLICK_VELOCITY ? flickDeform(velocity) : 0);
  lensFade.value = moved && keepFade;
  suppressClick = true;
  window.setTimeout(() => { suppressClick = false; }, 0);
  if (!willFly) {
    emit('reselect', next);
    return;
  }
  void fly(next, true, colored).then(() => commit(next));
};

const onClick = (value: T) => {
  if (props.disabled || suppressClick) return;
  if (value === props.modelValue) {
    emit('reselect', value);
    return;
  }
  void fly(value, true).then(() => commit(value));
};

const onKeydown = (event: KeyboardEvent) => {
  if (props.disabled || !props.items.length) return;
  const index = Math.max(0, props.items.findIndex((item) => item.value === props.modelValue));
  const last = props.items.length - 1;
  const target = {
    ArrowRight: Math.min(last, index + 1),
    ArrowDown: Math.min(last, index + 1),
    ArrowLeft: Math.max(0, index - 1),
    ArrowUp: Math.max(0, index - 1),
    Home: 0,
    End: last,
  }[event.key];
  if (target === undefined) return;
  event.preventDefault();
  const value = props.items[target]!.value;
  if (value === props.modelValue) return;
  void fly(value, true).then(() => commit(value));
};

watch(() => props.modelValue, async (value) => {
  await nextTick();
  if (!gesture && !restsOn(value)) void fly(value, false);
  focusActive();
});
watch(wrapped, async () => {
  await nextTick();
  stops.value = readStops();
  if (!gesture) placeOn(props.modelValue);
});
watch(() => props.items.map((item) => `${item.value}\u0000${item.label}\u0000${item.short ?? ''}`).join('\u0001'), async () => {
  await nextTick();
  observeItems();
  await fitAndMeasure();
});

const onResize = () => { void fitAndMeasure(); };
onMounted(() => {
  void nextTick(() => {
    void fitAndMeasure();
    observer = new ResizeObserver(() => { void fitAndMeasure(); });
    observeItems();
    const first = props.items[0]?.value;
    if (props.intro && first !== undefined && first !== props.modelValue && !reducedMotion()) {
      placeOn(first);
      void nextTick(() => requestAnimationFrame(() => { void fly(props.modelValue, false, first); }));
    }
  });
  void fontsReady().then(() => fitAndMeasure());
  window.addEventListener('resize', onResize);
});
onBeforeUnmount(() => {
  clearGesture();
  flyToken += 1;
  window.clearTimeout(colorTimer);
  for (const animation of [...flying.splice(0), ...morphing.splice(0)]) animation.cancel();
  elastic?.cancel();
  cancelAnimationFrame(fadeFrame);
  observer?.disconnect();
  window.removeEventListener('resize', onResize);
});
</script>

<template>
  <div
    ref="track"
    :class="['segment-track', `is-${variant}`, {
      'is-dragging': dragging, 'is-settling': settling, 'is-lens-fade': lensFade, 'is-compact': compactNow, 'is-disabled': disabled, 'is-fill': fill,
      'is-wrapped': wrapped,
      'is-icon-only': iconOnly,
      'has-lens': thumbLens.active.value,
      'is-lifted': lifted,
      'is-parked': parked,
    }]"
    :style="trackStyle"
    role="radiogroup"
    :aria-label="ariaLabel"
    :aria-disabled="disabled || undefined"
    @pointerenter="primeLens"
    @pointerdown="onDown"
    @pointermove="onMove"
    @pointerup="onUp"
    @pointercancel="onUp"
    @lostpointercapture="onUp"
    @keydown="onKeydown"
  >
    <span v-if="variant === 'glass'" ref="glassEl" class="segment-glass" aria-hidden="true" :style="rimLens.style('var(--glass-blur)')" />
    <span ref="thumbEl" class="segment-thumb" aria-hidden="true"><span ref="plateEl" class="segment-plate" /></span>
    <button
      v-for="(item, index) in items"
      :key="String(item.value)"
      type="button"
      role="radio"
      :data-value="String(item.value)"
      :class="['segment-item', { 'is-lensed': lensValue !== null && item.value === lensValue, 'is-offrow': offRow(index) }]"
      :style="itemLeft(index)"
      :aria-checked="item.value === modelValue"
      :aria-label="iconOnly ? item.label : fullTitle(item)"
      :title="iconOnly ? item.label : fullTitle(item)"
      :disabled="disabled"
      :tabindex="item.value === modelValue || (parked && item.value === placeholder) ? 0 : -1"
      @click="onClick(item.value)"
    >
      <slot :item="item" :active="false">
        <Icon v-if="item.icon" :name="item.icon" :size="iconOnly ? 17 : 15" />
        <span v-if="!iconOnly" class="seg-label" :data-label="shown(item)">{{ shown(item) }}</span>
      </slot>
    </button>
    <span class="segment-ink" aria-hidden="true" data-ash-skip>
      <span
        v-for="(stop, index) in stops"
        :key="String(stop.value)"
        :class="['segment-ink-item', { 'is-lensed': lensValue !== null && stop.value === lensValue }]"
        :style="{ left: `${stop.left}px`, width: `${stop.width}px`, top: `${stop.top ?? 0}px`, height: `${stop.height ?? 0}px` }"
      >
        <slot v-if="items[index]" :item="items[index]!" :active="true">
          <Icon v-if="items[index]!.icon" :name="items[index]!.icon!" :size="iconOnly ? 17 : 15" />
          <span v-if="!iconOnly">{{ shown(items[index]!) }}</span>
        </slot>
      </span>
    </span>
    <template v-if="thumbLens.active.value">
      <span ref="lensEl" class="segment-lens" aria-hidden="true" :style="thumbLens.style()" />
      <span ref="rimEl" class="segment-lens-rim" aria-hidden="true"><span ref="rimShape" class="segment-rim-shape" /></span>
    </template>
  </div>
</template>

<style scoped>
.segment-track {
  --seg-pad: 3px;
  --seg-dur: 300ms;
  --seg-ease: cubic-bezier(.3, 1.25, .4, 1);
  --seg-grow: 0px;
  --seg-clip-extra: 0px;
  position: relative;
  display: inline-flex;
  max-width: 100%;
  align-items: stretch;
  padding: var(--seg-pad);
  overflow: hidden;
  isolation: isolate;
  border-radius: 999px;
  user-select: none;
  touch-action: none;
}
.segment-track.is-dragging { --seg-grow: calc(var(--thumb-w) * .04); }
.segment-track.is-inset { background: var(--cap-track); box-shadow: var(--cap-track-shadow); }
.segment-track.is-glass { box-shadow: var(--glass-outline), var(--glass-shadow); }
.segment-glass {
  position: absolute;
  z-index: -1;
  inset: 0;
  border-radius: 999px;
  background: linear-gradient(180deg, var(--glass-sheen), transparent 60%), var(--glass);
  -webkit-backdrop-filter: var(--glass-blur);
  backdrop-filter: var(--glass-blur);
  pointer-events: none;
}
.segment-track.is-glass::after {
  content: '';
  position: absolute;
  z-index: 0;
  inset: 0;
  padding: 1px;
  border-radius: inherit;
  background: var(--glass-edge);
  -webkit-mask: linear-gradient(#000 0 0) content-box exclude, linear-gradient(#000 0 0);
  mask: linear-gradient(#000 0 0) content-box exclude, linear-gradient(#000 0 0);
  pointer-events: none;
}
@media (prefers-reduced-transparency: reduce) {
  .segment-glass { background: var(--mat-glass-strong); -webkit-backdrop-filter: none; backdrop-filter: none; }
}
.segment-track.has-lens { overflow: visible; }
.segment-track.is-fill { display: flex; }
.segment-track.is-fill .segment-item { flex: 1 1 0; min-width: max-content; }
.segment-track.is-inset:not(.is-fill) .segment-item { flex: 0 0 auto; }
.segment-track.is-wrapped { display: flex; width: 100%; flex-wrap: wrap; border-radius: 20px; }

.segment-item {
  position: relative;
  z-index: 1;
  display: inline-flex;
  min-width: 0;
  min-height: 32px;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 5px 15px;
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: var(--muted);
  font: inherit;
  font-size: var(--fs-sm);
  font-weight: 500;
  line-height: 1.2;
  white-space: nowrap;
  cursor: grab;
  -webkit-mask-image: linear-gradient(90deg,
    #000 calc(var(--thumb-l) - var(--seg-grow) - var(--item-l, 0px)),
    transparent calc(var(--thumb-l) - var(--seg-grow) - var(--item-l, 0px)),
    transparent calc(var(--thumb-l) + var(--thumb-w) + var(--seg-grow) - var(--item-l, 0px)),
    #000 calc(var(--thumb-l) + var(--thumb-w) + var(--seg-grow) - var(--item-l, 0px)));
  mask-image: linear-gradient(90deg,
    #000 calc(var(--thumb-l) - var(--seg-grow) - var(--item-l, 0px)),
    transparent calc(var(--thumb-l) - var(--seg-grow) - var(--item-l, 0px)),
    transparent calc(var(--thumb-l) + var(--thumb-w) + var(--seg-grow) - var(--item-l, 0px)),
    #000 calc(var(--thumb-l) + var(--thumb-w) + var(--seg-grow) - var(--item-l, 0px)));
}
.segment-item:hover:not(:disabled) { color: var(--ink); }
.segment-item.is-offrow { -webkit-mask-image: none; mask-image: none; }
.segment-track.is-parked:not(.is-dragging):not(.is-settling) .segment-item { -webkit-mask-image: none; mask-image: none; }
.segment-track.is-parked:not(.is-dragging):not(.is-settling) .segment-ink { opacity: 0; }
.segment-track.is-parked:not(.is-dragging):not(.is-settling) .segment-plate { opacity: .5; }
.seg-label { display: inline-grid; }
.seg-label::after {
  content: attr(data-label);
  height: 0;
  overflow: hidden;
  font-weight: 600;
  visibility: hidden;
}
.segment-item:focus-visible { outline: none; }
.segment-track.is-dragging .segment-item { cursor: grabbing; }
.segment-item:disabled { cursor: not-allowed; }

.segment-thumb {
  position: absolute;
  z-index: 0;
  top: var(--thumb-t);
  left: var(--thumb-l);
  width: var(--thumb-w);
  height: var(--thumb-h);
  pointer-events: none;
  opacity: var(--thumb-vis, 0);
}
.segment-plate {
  position: absolute;
  inset: 0;
  border-radius: 999px;
  background: var(--cap-thumb);
  box-shadow: var(--cap-thumb-rim);
  transition: scale var(--seg-dur) var(--seg-ease);
}
.segment-track.is-bare .segment-plate { background: var(--cap-glass-thumb); box-shadow: var(--cap-glass-thumb-rim); }
.segment-track.is-glass .segment-plate { background: var(--glass-tab-selected); box-shadow: var(--glass-tab-selected-rim); }

.segment-ink {
  position: absolute;
  z-index: 2;
  inset: 0;
  color: var(--cap-ink);
  font-weight: 600;
  pointer-events: none;
  clip-path: inset(calc(var(--thumb-t) - var(--seg-clip-extra)) calc(100% - var(--thumb-l) - var(--thumb-w) - var(--seg-grow))
    calc(100% - var(--thumb-t) - var(--thumb-h) - var(--seg-clip-extra)) calc(var(--thumb-l) - var(--seg-grow)) round 999px);
}
.segment-lens, .segment-lens-rim {
  position: absolute;
  z-index: 3;
  top: var(--lens-y);
  left: var(--lens-x);
  width: var(--lens-w);
  height: var(--lens-h);
  border-radius: 999px;
  pointer-events: none;
}
.segment-lens { opacity: 0; }
.segment-lens { visibility: hidden; transition: visibility 0s linear 320ms; }
.segment-track.is-lifted .segment-lens { visibility: visible; transition-delay: 0s; }
.segment-lens-rim { opacity: var(--thumb-vis, 0); }
.segment-rim-shape {
  position: absolute;
  inset: 0;
  border-radius: inherit;
  opacity: 0;
  box-shadow: var(--lens-glass-rim);
}
.segment-rim-shape::before {
  content: '';
  position: absolute;
  inset: 0;
  padding: 1.2px;
  border-radius: inherit;
  background: var(--lens-glass-specular);
  -webkit-mask: linear-gradient(#000 0 0) content-box exclude, linear-gradient(#000 0 0);
  mask: linear-gradient(#000 0 0) content-box exclude, linear-gradient(#000 0 0);
}
.segment-track.is-lifted .segment-lens, .segment-track.is-lifted .segment-rim-shape { opacity: 1; }
.segment-track.has-lens.is-lifted .segment-plate { opacity: 0; }
.segment-track.has-lens.is-dragging { --seg-clip-extra: 0px; }
.segment-ink-item {
  position: absolute;
  top: var(--seg-pad);
  bottom: var(--seg-pad);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 5px 15px;
  font-size: var(--fs-sm);
  line-height: 1.2;
  white-space: nowrap;
}

.segment-item { transition: color var(--dur-fast) ease; }
.segment-track.is-lens-fade .segment-ink-item, .segment-track.is-lens-fade .segment-item {
  transition: opacity 140ms ease, color var(--dur-fast) ease;
}
.segment-track.is-dragging .segment-ink, .segment-track.is-settling .segment-ink { clip-path: none; }
.segment-track.is-dragging .segment-ink-item, .segment-track.is-settling .segment-ink-item { opacity: 0; }
.segment-track.is-dragging .segment-ink-item.is-lensed, .segment-track.is-settling .segment-ink-item.is-lensed { opacity: 1; }
.segment-track.is-dragging .segment-item, .segment-track.is-settling .segment-item { -webkit-mask-image: none; mask-image: none; }
.segment-track.is-dragging .segment-item.is-lensed, .segment-track.is-settling .segment-item.is-lensed { opacity: 0; }

.segment-track.is-dragging:not(.has-lens) .segment-plate { scale: 1.05 1.1; }
.segment-track.is-dragging { --seg-clip-extra: var(--seg-pad); }

.segment-track:has(.segment-item:focus-visible) .segment-plate { filter: brightness(1.18); }

.segment-track.is-compact .segment-item,
.segment-track.is-compact .segment-ink-item { padding: 3px 13px; font-size: var(--fs-xs); }
.segment-track.is-compact .segment-item { min-height: 30px; }
.segment-track.is-icon-only .segment-item,
.segment-track.is-icon-only .segment-ink-item { padding-inline: 11px; }
.segment-track.is-disabled { opacity: .55; }

@media (prefers-reduced-motion: reduce) {
  .segment-plate { transition: none; }
}
</style>
