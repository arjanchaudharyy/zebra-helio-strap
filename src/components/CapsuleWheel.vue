<script setup lang="ts" generic="T extends string | number">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { fontsReady } from '../lib/fontsReady';
import Icon, { type IconName } from './Icon.vue';
import { useGlassLens } from '../composables/useGlassLens';
import { lensSpan, RESTING } from '../lib/glassGroup';
import { useGlassMember } from '../composables/useGlassGroup';
import { lessMotion } from '../lib/motion/lessMotion';

export type WheelItem<T extends string | number> = { value: T; label: string; icon?: IconName; image?: string };

const props = withDefaults(defineProps<{
  items: WheelItem<T>[];
  modelValue: T;
  ariaLabel?: string;
  orientation?: 'horizontal' | 'vertical';
  span?: number;
  iconOnly?: boolean;
  disabled?: boolean;
  loop?: boolean;
  fitPeek?: number;
  variant?: 'inset' | 'bare';
  activatable?: boolean;
  plain?: boolean;
}>(), {
  plain: false,
  orientation: 'horizontal',
  span: 148,
  iconOnly: false,
  disabled: false,
  loop: false,
  fitPeek: undefined,
  variant: 'inset',
  activatable: false,
});

const emit = defineEmits<{ 'update:modelValue': [value: T]; activate: [] }>();

const root = ref<HTMLElement | null>(null);
const refractEl = ref<HTMLElement | null>(null);
const refract = useGlassLens(refractEl, 'thumb', !props.plain);
const itemEls = ref<HTMLElement[]>([]);
const sizes = ref<number[]>([]);
const vertical = computed(() => props.orientation === 'vertical');
const count = () => props.items.length;
const looping = () => props.loop && count() > 2;
const mod = (value: number, n: number) => ((value % n) + n) % n;

const indexOf = (value: T) => Math.max(0, props.items.findIndex((item) => item.value === value));
const pos = ref(indexOf(props.modelValue));
let target = pos.value;
let velocity = 0;
let raf = 0;
let lastTs = 0;
const dragging = ref(false);
const animating = ref(false);

const GAP = 12;
const reducedMotion = () => lessMotion();

const measure = () => {
  sizes.value = itemEls.value.map((el) => (vertical.value ? el.offsetHeight : el.offsetWidth));
};
const centers = computed(() => {
  const out: number[] = [];
  let cursor = 0;
  sizes.value.forEach((size, index) => {
    const half = size / 2;
    cursor += index === 0 ? half : (sizes.value[index - 1]! / 2) + GAP + half;
    out.push(cursor);
  });
  return out;
});
const circumference = computed(() => sizes.value.reduce((sum, size) => sum + size + GAP, 0));

const arcAt = (p: number): number => {
  const list = centers.value;
  if (!list.length) return 0;
  const last = list.length - 1;
  if (looping()) {
    const n = list.length;
    const turns = Math.floor(p / n);
    const r = p - turns * n;
    const lo = Math.floor(r);
    const next = lo + 1 < n ? list[lo + 1]! : list[0]! + circumference.value;
    return turns * circumference.value + list[lo]! + (r - lo) * (next - list[lo]!);
  }
  if (p <= 0) return list[0]! + p * ((list[1] ?? list[0]! + 60) - list[0]!);
  if (p >= last) return list[last]! + (p - last) * (list[last]! - (list[last - 1] ?? list[last]! - 60));
  const lo = Math.floor(p);
  return list[lo]! + (p - lo) * (list[lo + 1]! - list[lo]!);
};
const posAt = (arc: number): number => {
  const list = centers.value;
  if (list.length < 2) return 0;
  const last = list.length - 1;
  if (looping()) {
    const n = list.length;
    const c = circumference.value;
    const turns = Math.floor((arc - list[0]!) / c);
    const a = arc - turns * c;
    let lo = 0;
    while (lo < last && list[lo + 1]! <= a) lo += 1;
    const next = lo + 1 < n ? list[lo + 1]! : list[0]! + c;
    return turns * n + lo + (a - list[lo]!) / (next - list[lo]!);
  }
  if (arc <= list[0]!) return (arc - list[0]!) / (list[1]! - list[0]!);
  if (arc >= list[last]!) return last + (arc - list[last]!) / (list[last]! - list[last - 1]!);
  let lo = 0;
  while (lo < last - 1 && list[lo + 1]! < arc) lo += 1;
  return lo + (arc - list[lo]!) / (list[lo + 1]! - list[lo]!);
};
const liveSpan = computed(() => {
  if (props.fitPeek === undefined || vertical.value || !sizes.value.length) return props.span;
  return Math.round(lensSize.value + 22 + props.fitPeek * 2);
});
const radius = computed(() => liveSpan.value * 0.62);

const clampPos = (value: number) => (looping() ? value : Math.min(count() - 1, Math.max(0, value)));
const rubber = (value: number) => {
  if (looping()) return value;
  const last = count() - 1;
  if (value < 0) return value / 3;
  if (value > last) return last + (value - last) / 3;
  return value;
};

const itemStyle = (index: number) => {
  let offset = (centers.value[index] ?? 0) - arcAt(pos.value);
  if (looping() && circumference.value > 0) offset -= circumference.value * Math.round(offset / circumference.value);
  const rad = Math.max(-1.75, Math.min(1.75, offset / radius.value));
  const angle = rad * 180 / Math.PI;
  const shift = radius.value * Math.sin(rad);
  const depth = radius.value * (Math.cos(rad) - 1);
  const far = Math.min(1, Math.abs(rad) / 1.5);
  const move = vertical.value
    ? `translate3d(-50%, calc(-50% + ${shift}px), ${depth}px) rotateX(${-angle}deg)`
    : `translate3d(calc(-50% + ${shift}px), -50%, ${depth}px) rotateY(${angle}deg)`;
  const width = sizes.value[index] || 40;
  const on = Math.max(0, 1 - Math.abs(offset) / (width * 0.8));
  return {
    transform: move,
    opacity: Math.abs(rad) >= 1.6 ? 0 : 1 - far * 0.75,
    zIndex: 100 - Math.round(Math.abs(rad) * 20),
    '--on': on.toFixed(3),
  };
};

const lensSize = computed(() => lensSpan(sizes.value, looping() ? pos.value : clampPos(pos.value), looping()));
useGlassMember('wheel', () => refractEl.value, () => ({ ...RESTING, active: dragging.value || animating.value }));
const lensStyle = computed(() => {
  if (vertical.value) return { height: `${lensSize.value + 14}px` };
  return { width: `${lensSize.value + (props.iconOnly ? 14 : 22)}px` };
});

const settleIndex = () => (looping() ? mod(Math.round(target), count()) : Math.round(clampPos(target)));
const settle = () => {
  const item = props.items[settleIndex()];
  if (item && item.value !== props.modelValue) emit('update:modelValue', item.value);
};

const tick = (ts: number) => {
  const dt = Math.min(34, lastTs ? ts - lastTs : 16.7) / 1000;
  lastTs = ts;
  const omega = 22;
  const x = pos.value - target;
  const accel = -omega * omega * x - 2 * omega * velocity;
  velocity += accel * dt;
  pos.value += velocity * dt;
  if (!committed && Math.abs(pos.value - target) < 0.01 && Math.abs(velocity) < 0.3) {
    committed = true;
    settle();
  }
  if (Math.abs(pos.value - target) < 0.002 && Math.abs(velocity) < 0.01) {
    pos.value = target;
    velocity = 0;
    raf = 0;
    lastTs = 0;
    animating.value = false;
    settle();
    return;
  }
  raf = requestAnimationFrame(tick);
};
let committed = false;
const animateTo = (index: number) => {
  target = clampPos(Math.round(index));
  committed = false;
  if (reducedMotion()) {
    pos.value = target;
    settle();
    return;
  }
  if (!raf) {
    animating.value = true;
    raf = requestAnimationFrame(tick);
  }
};
const turnTo = (index: number) => {
  if (!looping()) { animateTo(index); return; }
  const n = count();
  animateTo(index + n * Math.round((pos.value - index) / n));
};

let gesture: { id: number; hit: number; start: number; startArc: number; last: number; time: number; v: number; moved: boolean } | null = null;
const coord = (event: PointerEvent) => (vertical.value ? event.clientY : event.clientX);
const pxPerItem = () => {
  const n = count();
  const p = looping() ? mod(Math.round(pos.value), n) : Math.round(Math.min(n - 1, Math.max(0, pos.value)));
  const list = centers.value;
  const next = list[p + 1] ?? list[p - 1];
  return next === undefined || list[p] === undefined ? 60 : Math.abs(next - list[p]!);
};
const layoutScale = () => {
  const el = root.value;
  if (!el || !el.offsetWidth) return 1;
  return el.getBoundingClientRect().width / el.offsetWidth || 1;
};

const onDown = (event: PointerEvent) => {
  if (props.disabled || event.button !== 0 || !event.isPrimary || !root.value) return;
  cancelAnimationFrame(raf);
  raf = 0;
  animating.value = false;
  lastTs = 0;
  velocity = 0;
  const hit = (event.target as Element).closest<HTMLElement>('[data-wheel-index]');
  gesture = { id: event.pointerId, hit: hit ? Number(hit.dataset.wheelIndex) : -1, start: coord(event), startArc: arcAt(pos.value), last: coord(event), time: event.timeStamp, v: 0, moved: false };
  root.value.setPointerCapture(event.pointerId);
};
const onMove = (event: PointerEvent) => {
  const g = gesture;
  if (!g || g.id !== event.pointerId) return;
  const delta = (coord(event) - g.start) / layoutScale();
  if (!g.moved && Math.abs(delta) < 4) return;
  g.moved = true;
  dragging.value = true;
  const dt = Math.max(1, event.timeStamp - g.time);
  g.v = ((coord(event) - g.last) / layoutScale()) / dt;
  g.last = coord(event);
  g.time = event.timeStamp;
  pos.value = rubber(posAt(g.startArc - delta));
  event.preventDefault();
};
const onUp = (event: PointerEvent) => {
  const g = gesture;
  if (!g || g.id !== event.pointerId) return;
  gesture = null;
  dragging.value = false;
  if (root.value?.hasPointerCapture(event.pointerId)) root.value.releasePointerCapture(event.pointerId);
  if (event.type !== 'pointerup') {
    animateTo(pos.value);
    return;
  }
  if (!g.moved) {
    const current = looping() ? mod(Math.round(pos.value), count()) : Math.round(pos.value);
    if (g.hit >= 0 && g.hit !== current) turnTo(g.hit);
    else if (props.activatable && (g.hit === current || g.hit < 0)) { animateTo(pos.value); emit('activate'); }
    else if (count() === 2) turnTo(1 - current);
    else animateTo(pos.value);
    return;
  }
  const flick = event.timeStamp - g.time < 90 ? g.v : 0;
  const projected = pos.value - (flick * 140) / pxPerItem();
  velocity = -(flick * 1000) / pxPerItem() * 0.4;
  animateTo(projected);
};

let wheelAcc = 0;
const onWheel = (event: WheelEvent) => {
  if (props.disabled) return;
  const delta = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY;
  if (!delta) return;
  event.preventDefault();
  wheelAcc += delta;
  if (Math.abs(wheelAcc) < 40) return;
  const steps = Math.sign(wheelAcc);
  wheelAcc = 0;
  animateTo(Math.round(target) + steps);
};

const onKeydown = (event: KeyboardEvent) => {
  if (props.disabled) return;
  const current = Math.round(target);
  if (props.activatable && (event.key === 'Enter' || event.key === ' ')) {
    event.preventDefault();
    emit('activate');
    return;
  }
  const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[event.key];
  if (step !== undefined) {
    event.preventDefault();
    animateTo(current + step);
  } else if (event.key === 'Home' || event.key === 'End') {
    event.preventDefault();
    turnTo(event.key === 'Home' ? 0 : count() - 1);
  }
};

watch(() => props.modelValue, (value) => {
  if (gesture) return;
  const index = indexOf(value);
  const current = looping() ? mod(Math.round(target), count()) : Math.round(target);
  if (index !== current || Math.abs(pos.value - target) > 0.001) turnTo(index);
});
watch(() => props.items.length, () => {
  if (!looping() && target > count() - 1) animateTo(count() - 1);
});
watch(() => props.items.map((item) => item.label).join('\u0000'), async () => {
  await nextTick();
  measure();
});

let observer: ResizeObserver | null = null;
onMounted(() => {
  measure();
  void fontsReady().then(measure);
  observer = new ResizeObserver(measure);
  for (const el of itemEls.value) observer.observe(el);
  root.value?.addEventListener('wheel', onWheel, { passive: false });
});
onBeforeUnmount(() => {
  if (raf) settle();
  cancelAnimationFrame(raf);
  observer?.disconnect();
  root.value?.removeEventListener('wheel', onWheel);
});

const widestSpan = () => {
  if (props.fitPeek === undefined || vertical.value || !sizes.value.length) return props.span;
  return Math.round(Math.max(...sizes.value) + 22 + props.fitPeek * 2);
};
defineExpose({ measure, widestSpan });

const current = computed(() => props.items[indexOf(props.modelValue)]);
</script>

<template>
  <div
    ref="root"
    :class="['capsule-wheel', `is-${orientation}`, `is-${variant}`, {
      'is-dragging': dragging, 'is-animating': animating, 'is-fit': fitPeek !== undefined, 'is-icon-only': iconOnly, 'is-disabled': disabled,
      'has-lens': refract.active.value,
    }]"
    :style="vertical ? { height: `${span}px` } : { width: `${liveSpan}px` }"
    role="slider"
    tabindex="0"
    data-ash-skip
    :aria-label="ariaLabel"
    :aria-valuemin="0"
    :aria-valuemax="items.length - 1"
    :aria-valuenow="indexOf(modelValue)"
    :aria-valuetext="current?.label"
    :aria-disabled="disabled || undefined"
    @pointerdown="onDown"
    @pointermove="onMove"
    @pointerup="onUp"
    @pointercancel="onUp"
    @lostpointercapture="onUp"
    @keydown="onKeydown"
  >
    <span class="wheel-lens" aria-hidden="true" :style="lensStyle"></span>
    <div class="wheel-drum" aria-hidden="true">
      <span
        v-for="(item, index) in items"
        :key="String(item.value)"
        ref="itemEls"
        :data-wheel-index="index"
        class="wheel-item"
        :style="itemStyle(index)"
        :title="iconOnly ? item.label : undefined"
      >
        <img v-if="item.image" :src="item.image" alt="" class="wheel-image" draggable="false" />
        <Icon v-else-if="item.icon" :name="item.icon" :size="iconOnly ? 16 : 14" />
        <span v-if="!iconOnly" class="wheel-label">{{ item.label }}</span>
      </span>
    </div>
    <template v-if="refract.active.value">
      <span ref="refractEl" class="wheel-refract" aria-hidden="true" :style="[lensStyle, refract.style()]"></span>
      <span class="wheel-refract wheel-refract-rim" aria-hidden="true" :style="lensStyle"></span>
    </template>
  </div>
</template>

<style scoped>
.capsule-wheel {
  position: relative;
  display: inline-block;
  flex: 0 0 auto;
  height: 36px;
  overflow: hidden;
  border-radius: 999px;
  outline: none;
  cursor: grab;
  touch-action: none;
  user-select: none;
}
.capsule-wheel.is-inset { background: var(--cap-track); box-shadow: var(--cap-track-shadow); }
.wheel-drum {
  position: absolute;
  inset: 0;
  perspective: 420px;
  -webkit-mask-image: linear-gradient(90deg, transparent 0, #000 18%, #000 82%, transparent 100%);
  mask-image: linear-gradient(90deg, transparent 0, #000 18%, #000 82%, transparent 100%);
}
.capsule-wheel.is-vertical { width: auto; min-width: 120px; }
.capsule-wheel.is-vertical .wheel-drum {
  -webkit-mask-image: linear-gradient(180deg, transparent 0, #000 25%, #000 75%, transparent 100%);
  mask-image: linear-gradient(180deg, transparent 0, #000 25%, #000 75%, transparent 100%);
}
.capsule-wheel.is-fit { transition: width var(--dur-base) var(--ease-out); }
.capsule-wheel.is-fit.is-dragging, .capsule-wheel.is-fit.is-animating { transition: none; }
.capsule-wheel.is-dragging { cursor: grabbing; }
.capsule-wheel.is-disabled { opacity: .5; cursor: not-allowed; }
.capsule-wheel:focus-visible .wheel-lens { filter: brightness(1.18); }

.wheel-lens {
  position: absolute;
  top: 3px;
  bottom: 3px;
  left: 50%;
  display: flex;
  align-items: center;
  min-width: 30px;
  border-radius: 999px;
  background: var(--cap-thumb);
  box-shadow: var(--cap-thumb-rim);
  translate: -50% 0;
  transition: scale var(--dur-base) var(--ease-spring);
}
.capsule-wheel.is-bare .wheel-lens { background: var(--cap-glass-thumb); box-shadow: var(--cap-glass-thumb-rim); }
.is-vertical .wheel-lens { top: 50%; bottom: auto; left: 3px; right: 3px; translate: 0 -50%; min-width: 0; }
.capsule-wheel.is-dragging .wheel-lens { scale: 1.06 1.1; }
.wheel-refract {
  position: absolute;
  top: 3px;
  bottom: 3px;
  left: 50%;
  min-width: 30px;
  border-radius: 999px;
  translate: -50% 0;
  pointer-events: none;
  opacity: 0;
  visibility: hidden;
  transition: opacity 160ms ease, visibility 0s linear 160ms;
}
.wheel-refract-rim { box-shadow: var(--lens-glass-rim); }
.wheel-refract-rim::before {
  content: '';
  position: absolute;
  inset: 0;
  padding: 1.2px;
  border-radius: inherit;
  background: var(--lens-glass-specular);
  -webkit-mask: linear-gradient(#000 0 0) content-box exclude, linear-gradient(#000 0 0);
  mask: linear-gradient(#000 0 0) content-box exclude, linear-gradient(#000 0 0);
}
.is-vertical .wheel-refract { top: 50%; bottom: auto; left: 3px; right: 3px; translate: 0 -50%; min-width: 0; }
.capsule-wheel.is-dragging .wheel-refract, .capsule-wheel.is-animating .wheel-refract { opacity: 1; visibility: visible; transition-delay: 0s; }
.capsule-wheel.has-lens .wheel-lens { transition: scale var(--dur-base) var(--ease-spring), opacity 200ms ease; }
.capsule-wheel.has-lens:is(.is-dragging, .is-animating) .wheel-lens { scale: none; opacity: 0; transition: opacity 90ms ease; }

.wheel-item {
  position: absolute;
  top: 50%;
  left: 50%;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 0 11px;
  --on: 0;
  color: color-mix(in srgb, var(--cap-ink) calc(var(--on) * 100%), var(--muted));
  font-size: var(--fs-sm);
  font-weight: 500;
  line-height: 1;
  white-space: nowrap;
  backface-visibility: hidden;
}
.is-icon-only .wheel-item { padding: 0 7px; }
.wheel-label { display: inline-block; font-weight: 600; }
.wheel-image { width: 18px; height: 18px; flex: 0 0 18px; border-radius: 5px; pointer-events: none; }

</style>
