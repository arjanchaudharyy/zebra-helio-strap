<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, toRef, watch, type ComponentPublicInstance } from 'vue';
import GlyphTile from '../../GlyphTile.vue';
import Icon from '../../Icon.vue';
import StageWindow from './StageWindow.vue';
import StageMenu from './StageMenu.vue';
import { menuEnter, menuLeave } from './menuMorph';
import { ratioOf, ratioPresets, solve, STRIP_W, TAB_H, type Layout, type Placement } from '../../../lib/stage/layout';
import { FULL_SNAP, locate, type StageState } from '../../../lib/stage/state';
import type { SportClass, StageView } from '../../../lib/stage/prefs';
import { flyAll, SETTLE_EASE, snapshotOf, stopFlight, STAGE_MS, type FlySize, type Snapshot } from '../../../lib/motion/stageFlight';
import { OPEN_EASE } from '../../../lib/motion/timing';
import { holdResizes } from '../../../lib/motion/budget';
import { lessMotion } from '../../../lib/motion/lessMotion';
import { useWorkoutStage } from '../../../composables/useWorkoutStage';
import { useStageDrag } from '../../../composables/useStageDrag';
import { CLASSIC_COLUMNS, GROW_CARDS, PIP_CARDS, WIDE_CARDS, type StageCard } from '../workoutCards';
import { useMessages } from '../../../i18n';
import { stageMessages } from './stage.i18n';

const props = defineProps<{ cards: StageCard[]; sport: SportClass }>();
const t = useMessages(stageMessages);
const ids = computed(() => props.cards.map((card) => card.id));
const stage = useWorkoutStage(ids, toRef(props, 'sport'), (id) => WIDE_CARDS.has(id));
const cardOf = (id: string) => props.cards.find((card) => card.id === id);
const growing = computed(() => {
  const rigid = new Set(props.cards.filter((card) => card.rigid).map((card) => card.id));
  return rigid.size ? new Map([...GROW_CARDS].filter(([id]) => !rigid.has(id))) : GROW_CARDS;
});
const titleOf = (id: string) => cardOf(id)?.title ?? id;

const shell = ref<HTMLElement | null>(null);
const canvas = ref<HTMLElement | null>(null);
const ruler = ref<HTMLElement | null>(null);
const size = reactive({ width: 0, height: 0 });
const contentH = reactive<Record<string, number>>({});
const lastW = reactive<Record<string, number>>({});
const heightsAt = new Map<string, Map<number, number>>();
const HEIGHTS_KEPT = 8;
const remember = (id: string, w: number, h: number) => {
  const byWidth = heightsAt.get(id) ?? new Map<number, number>();
  byWidth.delete(w);
  byWidth.set(w, h);
  if (byWidth.size > HEIGHTS_KEPT) byWidth.delete(byWidth.keys().next().value!);
  heightsAt.set(id, byWidth);
};
const solveWith = (state: StageState, view: StageView = stage.view.value): Layout => {
  const input = {
    width: size.width || 1200, height: size.height || 640, state, cards: ids.value, contentH, lastW,
    pip: PIP_CARDS, view, columns: CLASSIC_COLUMNS, stripOpen: stage.stripOpen.value, front: stage.front.value, grow: growing.value,
  };
  const first = solve(input);
  let patched: Record<string, number> | null = null;
  for (const [id, p] of first.placements) {
    const known = heightsAt.get(id)?.get(Math.round(p.w));
    if (known != null && known !== contentH[id]) (patched ??= { ...contentH })[id] = known;
  }
  return patched ? solve({ ...input, contentH: patched }) : first;
};
const onMeasure = (id: string, h: number) => {
  const w = layout.value.placements.get(id)?.w;
  if (w != null) remember(id, Math.round(w), h);
  if (contentH[id] !== h) contentH[id] = h;
};
const classic = computed(() => stage.view.value === 'classic');
const isFull = computed(() => stage.state.value.stage.ids.length === 1 && stage.state.value.stage.snap === FULL_SNAP);

const els = new Map<string, HTMLElement>();
const bind = (id: string) => (ref: Element | ComponentPublicInstance | null) => {
  const el = ref && '$el' in ref ? (ref.$el as HTMLElement) : (ref as HTMLElement | null);
  if (el) els.set(id, el);
  else els.delete(id);
};

const menu = ref<string | null>(null);
let reopenTimer = 0;
const layout = computed<Layout>(() => {
  const out = solveWith(stage.draft.value ?? stage.state.value, stage.view.value);
  const pin = drag.pinned();
  if (pin) out.placements.set(pin.id, pin.p);
  return out;
});
const drag = useStageDrag({ stage, layout, canvas, size, element: (id) => els.get(id), solveWith, onStart: () => { closeMenu(); } });

/* ── FLIP ─────────────────────────────── */
const moved = (a: Placement | undefined, b: Placement) =>
  !a || a.x !== b.x || a.y !== b.y || a.w !== b.w || a.h !== b.h || a.scale !== b.scale || a.tilt !== b.tilt || a.opacity !== b.opacity || a.mode !== b.mode;
let ready = false;
let sizeKey = '';
const heldH = ref(0);
let holdTimer = 0;
let releaseResizes = () => {};
const viewSpan = () => {
  const top = canvas.value?.getBoundingClientRect().top ?? 0;
  return { top: -top, bottom: -top + window.innerHeight };
};
const visualBox = (p: Placement) => ({ y: p.y, h: p.h * p.scale });
watch(layout, (next, prev) => {
  const key = `${size.width}x${size.height}`;
  const resized = key !== sizeKey;
  sizeKey = key;
  if (!prev || !ready || resized || drag.isInstant()) return;
  const pin = drag.pinned()?.id;
  const view = viewSpan();
  const off = (y: number, h: number) => y + h < view.top - 40 || y > view.bottom + 40;
  const shots = new Map<string, Snapshot | 'slide'>();
  const taken = new Map<string, Snapshot>();
  for (const [id, p] of next.placements) {
    const el = els.get(id);
    if (!el || id === pin || (!moved(prev.placements.get(id), p) && !el.style.translate && !el.style.scale)) continue;
    taken.set(id, snapshotOf(el));
  }
  for (const id of taken.keys()) stopFlight(els.get(id)!);
  for (const [id, shot] of taken) {
    const p = next.placements.get(id)!;
    const was = prev.placements.get(id);
    const from = shot.rect ?? (was ? { ...visualBox(was) } : null);
    const to = visualBox(p);
    if (from && off(from.y, from.h) && off(to.y, to.h)) continue;
    shots.set(id, from && off(from.y, from.h) && p.opacity > 0 ? 'slide' : shot);
  }
  if (!shots.size) return;
  if (shots.size > 1) {
    releaseResizes();
    releaseResizes = holdResizes(STAGE_MS + 400);
    window.setTimeout(releaseResizes, STAGE_MS);
  }
  heldH.value = Math.max(heldH.value, prev.canvasH, next.canvasH);
  clearTimeout(holdTimer);
  holdTimer = window.setTimeout(() => { heldH.value = 0; }, Math.round(STAGE_MS * 1.2) + 60);
  void nextTick(() => {
    const first = [...shots.keys()].map((id) => els.get(id)).find(Boolean);
    const radius = first ? parseFloat(getComputedStyle(first).borderTopLeftRadius) || 0 : 0;
    const flights: Parameters<typeof flyAll>[0][number][] = [];
    for (const [id, shot] of shots) {
      const el = els.get(id);
      const p = next.placements.get(id);
      if (!el || !p) continue;
      if (shot === 'slide') {
        if (!lessMotion()) el.animate([{ translate: '0 48px', opacity: 0 }, { translate: '0 0', opacity: Number(el.style.opacity || 1) }], { duration: STAGE_MS, easing: OPEN_EASE });
        continue;
      }
      const size: FlySize = { w: p.w, h: p.h, radius };
      flights.push({ el, from: shot, easing: drag.releasing.has(id) ? SETTLE_EASE : undefined, size });
    }
    flyAll(flights);
  });
}, { flush: 'pre' });
watch(layout, (next) => {
  for (const [id, p] of next.placements) if (p.mode === 'stage' && lastW[id] !== p.w) lastW[id] = p.w;
});

const activate = (id: string) => {
  if (drag.justDragged.value) return;
  const where = locate(stage.state.value, id);
  if (where?.at === 'strip') stage.bringGroup(where.group);
  stage.stripOpen.value = false;
};
const toggleMenu = (id: string) => {
  if (drag.justDragged.value) return;
  clearTimeout(reopenTimer);
  menu.value = menu.value === id ? null : id;
};
function closeMenu(focusGrip = false) {
  clearTimeout(reopenTimer);
  const id = menu.value;
  menu.value = null;
  if (focusGrip && id) void nextTick(() => els.get(id)?.querySelector<HTMLElement>('.window-grip')?.focus());
}
const MENU_W = 320;
const menuAt = computed(() => {
  const p = menu.value ? layout.value.placements.get(menu.value) : null;
  if (!p) return null;
  const grip = p.x + (p.w * p.scale) / 2;
  const x = Math.min(Math.max(8, grip - MENU_W / 2), size.width - MENU_W - 8);
  const y = p.y + 30;
  return { x, y, from: [Math.round(grip - 26 - x), p.y + 4 - y, 52, 18].join(',') };
});
// Measure the visible stage, including the fixed navigation on narrow windows.
function fitMenu(el: HTMLElement) {
  const surface = canvas.value;
  const grip = menu.value ? els.get(menu.value)?.querySelector<HTMLElement>('.window-grip') : null;
  const inner = el.querySelector<HTMLElement>('.menu-inner');
  if (!surface || !grip || !inner || !surface.offsetWidth) return;
  const stageRect = surface.getBoundingClientRect();
  const anchor = grip.getBoundingClientRect();
  const zoom = stageRect.width / surface.offsetWidth || 1;
  const main = document.getElementById('main-content')?.getBoundingClientRect();
  const topbar = document.querySelector('.app-topbar')?.getBoundingClientRect();
  const bottomNav = document.querySelector('.bottom-nav')?.getBoundingClientRect();
  const left = Math.max(0, stageRect.left, main?.left ?? 0) + 8;
  const right = Math.min(window.innerWidth, stageRect.right, main?.right ?? Infinity) - 8;
  const top = Math.max(0, main?.top ?? 0, topbar?.bottom ?? 0) + 8;
  const bottom = Math.min(window.innerHeight, main?.bottom ?? Infinity, bottomNav?.height ? bottomNav.top : Infinity) - 8;
  const width = Math.max(1, Math.min(MENU_W, (right - left) / zoom));
  el.style.width = `${width}px`;
  const height = Math.min(inner.scrollHeight * zoom, Math.max(0, bottom - top));
  const x = Math.max(left, Math.min(right - width * zoom, anchor.left + (anchor.width - width * zoom) / 2));
  const y = anchor.bottom + 6 + height <= bottom
    ? Math.max(top, anchor.bottom + 6)
    : Math.max(top, Math.min(bottom - height, anchor.top - 6 - height));
  el.style.left = `${(x - stageRect.left) / zoom}px`;
  el.style.top = `${(y - stageRect.top) / zoom}px`;
  el.style.setProperty('--menu-max-height', `${Math.max(0, bottom - top) / zoom}px`);
  el.dataset.from = [(anchor.left - x) / zoom, (anchor.top - y) / zoom, anchor.width / zoom, anchor.height / zoom].join(',');
}
const enterMenu = (el: Element, done: () => void) => {
  fitMenu(el as HTMLElement);
  menuEnter(el, done);
};
const refitMenu = () => {
  const el = canvas.value?.querySelector<HTMLElement>('.stage-menu');
  if (menu.value && el) fitMenu(el);
};

watch(menuAt, refitMenu, { flush: 'post' });

const menuSnap = computed(() => {
  const g = stage.state.value.stage;
  if (g.ids.length >= 3) return { snap: g.snap, exact: true };
  const presets = ratioPresets(g.ids.length, g.snap).slice(0, g.ids.length === 1 ? 2 : 3);
  const ratio = ratioOf(g);
  let best = 0;
  presets.forEach((r, i) => { if (Math.abs(r - ratio) < Math.abs(presets[best] - ratio)) best = i; });
  return { snap: best, exact: Math.abs(presets[best] - ratio) < 0.002 && g.span == null };
});
const act = (kind: 'solo' | 'full' | 'exitFull' | 'tuck') => {
  const id = menu.value;
  if (!id) return;
  closeMenu();
  if (kind === 'solo') stage.solo(id);
  else if (kind === 'tuck') stage.removeFromStage(id);
  else if (kind === 'full') stage.fullscreen(id);
  else stage.setSnap(1);
};
const pickSnap = (snap: number) => {
  const id = menu.value;
  if (!id) return;
  clearTimeout(reopenTimer);
  const quick = lessMotion();
  reopenTimer = window.setTimeout(() => {
    menu.value = null;
    stage.setSnap(snap);
    if (quick) return;
    reopenTimer = window.setTimeout(() => {
      if (!menu.value && !drag.dragging.value && stage.state.value.stage.ids.includes(id)) menu.value = id;
    }, STAGE_MS + 140);
  }, quick ? 0 : 150);
};

const switchView = (next: StageView) => {
  closeMenu();
  stage.setView(next);
  if (next !== 'stage') return;
  const top = shell.value?.getBoundingClientRect().top ?? 0;
  const sticky = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--sticky-top')) || 0;
  if (top < sticky) shell.value?.scrollIntoView({ behavior: lessMotion() ? 'auto' : 'smooth', block: 'start' });
};
const reset = () => { closeMenu(); stage.reset(); };

let stripTimer = 0;
const peek = (open: boolean) => {
  clearTimeout(stripTimer);
  if (!layout.value.stripAuto) return;
  if (open) stage.stripOpen.value = true;
  else if (stage.stripOpen.value) stripTimer = window.setTimeout(() => { stage.stripOpen.value = false; }, 320);
};
const onCanvasMove = (event: PointerEvent) => {
  if (!layout.value.stripAuto || classic.value || drag.dragging.value) return;
  const x = event.clientX - (canvas.value?.getBoundingClientRect().left ?? 0);
  if (!stage.stripOpen.value && x < 14) peek(true);
  else if (stage.stripOpen.value) peek(x < STRIP_W + 24);
};

const onKey = (event: KeyboardEvent) => {
  const target = event.target as HTMLElement;
  if (classic.value || target.closest('[data-stage-scroll], input, textarea, select, [role="menu"], .stage-head')) return;
  if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') {
    event.preventDefault();
    stage.cycle(event.key === 'ArrowRight' ? 1 : -1);
  } else if (event.key === 'Escape') {
    if (stage.stripOpen.value) stage.stripOpen.value = false;
    else if (isFull.value) stage.setSnap(1);
    else return;
    event.preventDefault();
  }
};
const onOutside = (event: PointerEvent) => {
  const target = event.target as HTMLElement;
  if (!target.closest('.stage-menu, .window-grip')) { if (menu.value) closeMenu(); else clearTimeout(reopenTimer); }
  if (hiddenOpen.value && !target.closest('.hidden-pop, .hidden-toggle')) hiddenOpen.value = false;
};
const onShow = (event: Event) => {
  const id = (event as CustomEvent<string>).detail;
  if (typeof id === 'string' && ids.value.includes(id) && !classic.value) stage.focusCard(id);
};

const hiddenOpen = ref(false);
const restore = (id: string) => { hiddenOpen.value = false; stage.focusCard(id); };

let observer: ResizeObserver | null = null;
onMounted(() => {
  observer = new ResizeObserver(() => {
    size.width = Math.round(canvas.value?.clientWidth ?? 0);
    size.height = Math.round(ruler.value?.clientHeight ?? 0);
    if (!ready && size.width) requestAnimationFrame(() => requestAnimationFrame(() => { ready = true; }));
  });
  if (canvas.value) observer.observe(canvas.value);
  if (ruler.value) observer.observe(ruler.value);
  document.addEventListener('pointerdown', onOutside, true);
  document.getElementById('main-content')?.addEventListener('scroll', refitMenu, { passive: true });
  window.addEventListener('resize', refitMenu);
  window.addEventListener('zb:stage-show', onShow);
});
onBeforeUnmount(() => {
  observer?.disconnect();
  clearTimeout(stripTimer);
  clearTimeout(reopenTimer);
  clearTimeout(holdTimer);
  releaseResizes();
  stage.draft.value = null;
  document.removeEventListener('pointerdown', onOutside, true);
  document.getElementById('main-content')?.removeEventListener('scroll', refitMenu);
  window.removeEventListener('resize', refitMenu);
  window.removeEventListener('zb:stage-show', onShow);
});
defineExpose({ focusCard: (id: string) => stage.focusCard(id) });
</script>

<template>
  <div
    ref="shell"
    class="stage-shell"
    :class="{ narrow: layout.narrow, classic }"
    :style="{ '--stage-ms': `${STAGE_MS}ms`, '--stage-ease': OPEN_EASE }"
    role="region"
    :aria-label="t.region"
    @keydown="onKey"
  >
    <header class="stage-head">
      <div class="head-title"><slot name="title" /></div>
      <div class="head-actions">
        <slot name="actions" />
        <span class="head-sep" aria-hidden="true"></span>
        <Transition name="head-fade">
          <div v-if="!classic && stage.state.value.hidden.length" class="hidden-wrap">
            <button class="head-button hidden-toggle" type="button" :aria-expanded="hiddenOpen" @click="hiddenOpen = !hiddenOpen">{{ t.hiddenCount(stage.state.value.hidden.length) }}</button>
            <Transition name="pop">
              <div v-if="hiddenOpen" class="hidden-pop" :title="t.restore">
                <button v-for="id in stage.state.value.hidden" :key="id" type="button" @click="restore(id)">
                  <GlyphTile :name="cardOf(id)?.icon ?? 'structured-data'" :tone="cardOf(id)?.tone" :size="18" />{{ titleOf(id) }}
                </button>
              </div>
            </Transition>
          </div>
        </Transition>
        <button class="head-button" type="button" :aria-pressed="classic" @click="switchView(classic ? 'stage' : 'classic')">
          {{ classic ? t.exposeExit : t.expose }}
        </button>
        <Transition name="head-fade">
          <button v-if="!classic" class="head-button" type="button" @click="reset">{{ t.reset }}</button>
        </Transition>
      </div>
    </header>

    <div class="stage-ruler" ref="ruler" aria-hidden="true"></div>
    <div ref="canvas" class="stage-canvas" :style="{ height: `${Math.max(layout.canvasH, heldH)}px` }" @pointermove="onCanvasMove" @pointerleave="peek(false)">
      <div v-if="layout.tabs.length" class="group-tabs" role="tablist" :aria-label="t.groupTabs" :style="{ height: `${TAB_H - 8}px` }">
        <button
          v-for="id in layout.tabs"
          :key="id"
          type="button"
          role="tab"
          :aria-selected="layout.placements.get(id)?.mode === 'stage'"
          @click="stage.setFront(id)"
        >{{ titleOf(id) }}</button>
      </div>

      <StageWindow
        v-for="card in cards"
        :key="card.id"
        :ref="bind(card.id)"
        :id="card.id"
        :title="card.title"
        :placement="layout.placements.get(card.id)!"
        :menu-open="menu === card.id"
        @grab="(event, kind, handle) => drag.grab(card.id, event, kind, handle)"
        @activate="activate(card.id)"
        @menu="toggleMenu(card.id)"
        @measure="(h) => onMeasure(card.id, h)"
      >
        <slot :name="card.id" />
      </StageWindow>

      <Transition name="head-fade">
        <button
          v-if="layout.stripAuto && !layout.stripShown && !classic"
          type="button"
          class="strip-handle glass-control"
          :aria-label="t.showStrip"
          :title="t.showStrip"
          @click="peek(true)"
        ><Icon name="chevron-right" :size="16" /></button>
      </Transition>

      <Transition :css="false" @enter="enterMenu" @leave="menuLeave">
        <StageMenu
          v-if="menu && menuAt"
          :key="menu"
          :title="titleOf(menu)"
          :x="menuAt.x"
          :y="menuAt.y"
          :from="menuAt.from"
          :count="stage.state.value.stage.ids.length"
          :snap="menuSnap.snap"
          :exact="menuSnap.exact"
          :full="isFull"
          @act="act"
          @snap="pickSnap"
          @close="closeMenu(true)"
        />
      </Transition>
    </div>
  </div>
</template>

<style scoped src="./StageShell.css"></style>
