<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue';
import { exemptFromSettle, onMotionEscape } from '../lib/motion/interrupt';
import { SPRINGS } from '../lib/motion/cards/spring';
import { lessMotion } from '../lib/motion/lessMotion';
import { whenFramesSteady } from '../lib/motion/steady';
import { paced, OPEN_EASE } from '../lib/motion/timing';
import { cardReplica, visibleWindow } from '../lib/motion/window';
import {
  POPOVER_RADIUS, SHADOW_BLEED, bleedInset, elastic, popoverFrames, reachable, releaseVelocity, shouldDismiss, springBack,
  type DragSample,
} from '../lib/motion/popoverMorph';
import { anchoredLeft } from '../lib/popoverPosition';
import { holdSource } from '../lib/motion/sourceHold';

const props = withDefaults(defineProps<{ anchor: HTMLElement | null; labelledby?: string; width?: number }>(), { labelledby: undefined, width: 340 });
const emit = defineEmits<{ close: [] }>();
const panel = ref<HTMLElement | null>(null);
const veil = ref<HTMLElement | null>(null);
const style = ref<Record<string, string>>({});
const ready = ref(false);
const settled = ref(false);
const drag = ref({ x: 0, y: 0 });
const dragging = ref(false);
const morphing = ref(false);
const MARGIN = 20;
const GAP = 8;
const OPEN_MS = paced(420);
const CLOSE_MS = paced(360);
const NEARLY_HIDDEN = 0.001;
let closing: Promise<void> | null = null;
let placed = { left: 0, top: 0, width: 0, height: 0 };
let replica: HTMLElement | null = null;
let releaseAnchor: () => void = () => undefined;

const place = () => {
  const box = props.anchor?.isConnected ? props.anchor.getBoundingClientRect() : null;
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const width = Math.min(props.width, vw - MARGIN * 2);
  const maxHeight = vh - MARGIN * 2;
  const height = Math.min(panel.value?.scrollHeight ?? 220, maxHeight);
  const anchor = box ?? new DOMRect(vw / 2, vh / 2, 0, 0);
  const left = anchoredLeft(anchor, width, vw, MARGIN);
  let top: number;
  if (anchor.bottom + GAP + height <= vh - MARGIN) top = anchor.bottom + GAP;
  else if (anchor.top - GAP - height >= MARGIN) top = anchor.top - GAP - height;
  else top = Math.min(Math.max(MARGIN, anchor.bottom + GAP), vh - MARGIN - height);
  placed = { left, top, width, height };
  style.value = { left: `${left}px`, top: `${top}px`, width: `${width}px`, maxHeight: `${maxHeight}px` };
};

const focusables = () => [...(panel.value?.querySelectorAll<HTMLElement>('button, a[href], input, textarea, [tabindex]:not([tabindex="-1"])') ?? [])]
  .filter((el) => !el.hasAttribute('disabled'));

const anchorRect = (): DOMRect | null => {
  const el = props.anchor;
  if (!el?.isConnected || !el.getClientRects().length) return null;
  const rect = el.getBoundingClientRect();
  return reachable(rect, { width: window.innerWidth, height: window.innerHeight }) ? rect : null;
};

const layers = (el: HTMLElement) => [...el.querySelectorAll<HTMLElement>(':scope > .pop-grip, :scope > .pop-body')];

const attachReplica = (el: HTMLElement, frames: Keyframe[], timing: KeyframeAnimationOptions) => {
  replica?.remove();
  replica = null;
  if (!props.anchor?.isConnected) return;
  const copy = cardReplica(props.anchor).el;
  copy.removeAttribute('data-source-hold');
  copy.querySelectorAll('[data-source-hold]').forEach((node) => node.removeAttribute('data-source-hold'));
  Object.assign(copy.style, { left: `${-el.clientLeft}px`, top: `${-el.clientTop}px`, zIndex: '4' });
  el.appendChild(copy);
  replica = copy;
  copy.animate(frames, { ...timing, easing: 'linear', fill: 'both' });
};

let resizer: ResizeObserver | null = null;
const open = async () => {
  place();
  await nextTick();
  place();
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  const el = panel.value;
  if (!el || closing) return;
  place();
  const parts = layers(el);
  let morph: Animation;
  if (lessMotion()) {
    morph = el.animate([{ opacity: NEARLY_HIDDEN }, { opacity: 1 }], { duration: 160, easing: 'ease-out', fill: 'backwards' });
    veil.value?.animate([{ opacity: NEARLY_HIDDEN }, { opacity: 1 }], { duration: 160, easing: 'ease-out', fill: 'backwards' });
  } else {
    const from = anchorRect() ?? { left: placed.left + placed.width / 2 - 20, top: placed.top, width: 40, height: 40 };
    const timing = { duration: OPEN_MS, fill: 'backwards' as const };
    morph = el.animate(popoverFrames(from, placed, placed, POPOVER_RADIUS, SHADOW_BLEED), { ...timing, easing: OPEN_EASE });
    attachReplica(el, [{ opacity: 1 }, { opacity: 0, offset: 0.3 }, { opacity: 0 }], timing);
    if (anchorRect()) releaseAnchor = holdSource(props.anchor);
    for (const part of parts) part.animate([{ opacity: NEARLY_HIDDEN }, { opacity: NEARLY_HIDDEN, offset: 0.4 }, { opacity: 1 }], { ...timing, easing: 'ease-out' });
    for (const item of el.querySelectorAll<HTMLElement>('[data-pop-item]')) {
      item.animate([{ transform: 'translateY(6px)' }, { transform: 'translateY(6px)', offset: 0.4 }, { transform: 'none' }],
        { ...timing, easing: 'cubic-bezier(.2, .9, .25, 1)' });
    }
    veil.value?.animate([{ opacity: NEARLY_HIDDEN }, { opacity: 1 }], { ...timing, easing: 'ease-out' });
  }
  const held = [...el.getAnimations({ subtree: true }), ...(veil.value?.getAnimations() ?? [])];
  for (const animation of held) animation.pause();
  morphing.value = !lessMotion();
  ready.value = true;
  focusables()[0]?.focus({ preventScroll: true });
  await whenFramesSteady(160);
  for (const animation of held) if (animation.playState === 'paused') animation.play();
  void morph.finished.then(() => {
    replica?.remove();
    replica = null;
    morphing.value = false;
    settled.value = true;
    place();
    resizer = new ResizeObserver(() => { if (!closing && !dragging.value) place(); });
    for (const child of el.children) resizer.observe(child);
  }, () => undefined);
};

const freezeDrag = (el: HTMLElement) => {
  const [x = 0, y = 0] = getComputedStyle(el).translate.split(/\s+/).map((v) => Number.parseFloat(v) || 0);
  for (const animation of el.getAnimations()) {
    if ((animation.effect as KeyframeEffect | null)?.getKeyframes().some((frame) => 'translate' in frame)) animation.cancel();
  }
  el.style.translate = `${x}px ${y}px`;
  drag.value = { x, y };
  return { x, y };
};

const close = (): Promise<void> => {
  if (closing) return closing;
  const el = panel.value;
  closing = (async () => {
    resizer?.disconnect();
    dragging.value = false;
    if (el && ready.value) {
      const offset = freezeDrag(el);
      const timing = { duration: CLOSE_MS, fill: 'forwards' as const };
      const veilNow = veil.value ? Number(getComputedStyle(veil.value).opacity) : 1;
      const parts = layers(el);
      const partsNow = parts.map((part) => Number(getComputedStyle(part).opacity));
      const to = anchorRect();
      const duration = lessMotion() ? 140 : to ? CLOSE_MS : 220;
      let shrink: Animation;
      if (lessMotion() || !to) {
        const { transform, opacity, clipPath } = getComputedStyle(el);
        const now = { transform, opacity, clipPath };
        const frames: Keyframe[] = [{ transform: now.transform, opacity: now.opacity, ...(now.clipPath === 'none' ? {} : { clipPath: now.clipPath }) },
          { transform: lessMotion() ? now.transform : 'scale(.94)', opacity: 0, ...(now.clipPath === 'none' ? {} : { clipPath: now.clipPath }) }];
        for (const animation of el.getAnimations({ subtree: true })) animation.cancel();
        replica?.remove();
        shrink = exemptFromSettle(el.animate(frames, { ...timing, duration, easing: 'ease-in' }));
      } else {
        const { transform, clipPath } = getComputedStyle(el);
        const now = { transform, clipPath };
        const seen = now.clipPath === 'none' ? null : visibleWindow(el);
        for (const animation of el.getAnimations({ subtree: true })) animation.cancel();
        replica?.remove();
        replica = null;
        morphing.value = true;
        const box = { ...placed, left: placed.left + offset.x, top: placed.top + offset.y };
        const frames = popoverFrames(seen ?? box, to, box);
        frames[0] = { ...frames[0], transform: now.transform === 'none' ? 'translate(0px, 0px)' : now.transform, clipPath: seen ? now.clipPath : bleedInset() };
        shrink = exemptFromSettle(el.animate(frames, { ...timing, easing: OPEN_EASE }));
        parts.forEach((part, i) => exemptFromSettle(part.animate([{ opacity: partsNow[i] }, { opacity: 0, offset: 0.3 }, { opacity: 0 }], { ...timing, easing: 'linear' })));
        attachReplica(el, [{ opacity: 0 }, { opacity: 0, offset: 0.4 }, { opacity: 1, offset: 0.8 }, { opacity: 1 }], timing);
      }
      if (veil.value) {
        for (const animation of veil.value.getAnimations()) animation.cancel();
        exemptFromSettle(veil.value.animate([{ opacity: veilNow }, { opacity: veilNow * 0.88, offset: 0.4 }, { opacity: 0 }], { ...timing, duration, easing: 'linear' }));
      }
      await shrink.finished.catch(() => undefined);
    }
    releaseAnchor();
    props.anchor?.focus({ preventScroll: true });
    emit('close');
  })();
  return closing;
};

let start = { x: 0, y: 0, dx: 0, dy: 0 };
let samples: DragSample[] = [];
const onGripDown = (event: PointerEvent) => {
  const el = panel.value;
  if (event.button !== 0 || closing || !el || !settled.value) return;
  event.preventDefault();
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  const now = freezeDrag(el);
  dragging.value = true;
  start = { x: event.clientX, y: event.clientY, dx: now.x, dy: now.y };
  samples = [{ t: event.timeStamp, x: now.x, y: now.y }];
};
const onGripMove = (event: PointerEvent) => {
  if (!dragging.value) return;
  const { left, top, width, height } = placed;
  const raw = { x: start.dx + event.clientX - start.x, y: start.dy + event.clientY - start.y };
  const x = elastic(raw.x, MARGIN - left, Math.max(MARGIN - left, window.innerWidth - MARGIN - width - left));
  const y = elastic(raw.y, MARGIN - top, Math.max(MARGIN - top, window.innerHeight - MARGIN - height - top));
  drag.value = { x, y };
  samples.push({ t: event.timeStamp, ...raw });
  if (samples.length > 24) samples.splice(0, samples.length - 24);
};
const onGripUp = (event: PointerEvent) => {
  if (!dragging.value) return;
  dragging.value = false;
  (event.currentTarget as HTMLElement).releasePointerCapture?.(event.pointerId);
  const el = panel.value;
  if (!el) return;
  const offset = { ...drag.value };
  const pulled = samples[samples.length - 1]?.y ?? offset.y;
  const velocity = releaseVelocity(samples, event.timeStamp);
  samples = [];
  if (shouldDismiss(pulled, velocity.y)) { void close(); return; }
  if (Math.hypot(offset.x, offset.y) < 0.5) return;
  drag.value = { x: 0, y: 0 };
  el.style.translate = '0px 0px';
  if (lessMotion()) return;
  const { easing, duration } = springBack(SPRINGS.settle, offset, velocity);
  el.animate([{ translate: `${offset.x}px ${offset.y}px` }, { translate: '0px 0px' }], { duration, easing });
};

const onKey = (event: KeyboardEvent) => {
  if (event.key !== 'Tab') return;
  const items = focusables();
  if (!items.length) return;
  const first = items[0]!;
  const last = items[items.length - 1]!;
  if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
  else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
};

const releaseEscape = onMotionEscape(() => { void close(); return true; });
const onResize = () => { if (settled.value && !closing) place(); };
onMounted(() => {
  window.addEventListener('resize', onResize);
  void open();
});
onBeforeUnmount(() => {
  releaseEscape();
  releaseAnchor();
  resizer?.disconnect();
  replica?.remove();
  window.removeEventListener('resize', onResize);
});
defineExpose({ close });
</script>

<template>
  <Teleport to="body">
    <div :class="['pop-layer', { ready }]">
      <div ref="veil" class="pop-veil" aria-hidden="true" @click="close"></div>
      <div ref="panel" :class="['pop-panel', { settled, dragging, morphing }]" role="dialog" aria-modal="true" :aria-labelledby="labelledby"
        :style="{ ...style, translate: `${drag.x}px ${drag.y}px` }" @keydown="onKey">
        <div class="pop-grip" aria-hidden="true"
          @pointerdown="onGripDown" @pointermove="onGripMove" @pointerup="onGripUp" @pointercancel="onGripUp"><i></i></div>
        <div class="pop-body">
          <slot :close="close" />
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.pop-layer { position: fixed; inset: 0; z-index: 2100; }
.pop-layer:not(.ready) { visibility: hidden; }
.pop-veil { position: absolute; inset: 0; background: color-mix(in srgb, #000 24%, transparent);
  -webkit-backdrop-filter: blur(14px) saturate(1.1); backdrop-filter: blur(14px) saturate(1.1); }
:root[data-theme='light'] .pop-veil { background: color-mix(in srgb, #0b1210 12%, transparent); }
.pop-panel { position: absolute; display: grid; grid-template-rows: auto minmax(0, 1fr); padding: 0; border: 1px solid var(--mat-glass-line); border-radius: 22px;
  background: var(--mat-glass-strong); -webkit-backdrop-filter: var(--mat-glass-blur); backdrop-filter: var(--mat-glass-blur);
  box-shadow: var(--mat-glass-shadow), 0 30px 60px -30px rgba(0, 0, 0, .7); color: var(--ink); will-change: transform, clip-path; overflow: hidden; }
.pop-panel.settled:not(.dragging) { transition: left 340ms cubic-bezier(.2, .8, .2, 1), top 340ms cubic-bezier(.2, .8, .2, 1); }
.pop-panel.morphing { overflow: visible; }
.pop-body { display: grid; gap: 8px; min-height: 0; padding: 2px 16px 16px; overflow: auto; overscroll-behavior: contain; scrollbar-width: none; }
.pop-grip { display: grid; place-items: center; height: 16px; cursor: grab; touch-action: none; }
.pop-grip i { width: 38px; height: 4px; border-radius: 999px; background: color-mix(in srgb, var(--ink) 18%, transparent); transition: background 160ms ease, width 200ms ease; }
.pop-grip:hover i { width: 46px; background: color-mix(in srgb, var(--ink) 34%, transparent); }
.pop-panel.dragging .pop-grip { cursor: grabbing; }
.pop-panel.dragging .pop-grip i { width: 46px; background: var(--accent); }
@media (prefers-reduced-motion: reduce) { .pop-panel.settled:not(.dragging) { transition: none; } }
</style>
