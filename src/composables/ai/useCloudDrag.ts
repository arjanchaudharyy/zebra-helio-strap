import { computed, onBeforeUnmount, reactive, ref } from 'vue';
import { reducedMotion, SPRINGS, springCurve } from '../../lib/motion/cards';

const START_PX = 6;
const FLICK_VY = -0.55;
const MAX_TILT = 8;
const offsets = reactive(new Map<string, { x: number; y: number }>());

export interface CloudDragOptions {
  slotOf: (id: string) => HTMLElement | null;
  canFlick: (id: string) => boolean;
  onTap: (id: string, el: HTMLElement) => void;
  onFlick: (id: string) => void;
  onLand?: (id: string, dx: number, dy: number) => void;
  grabAt?: (id: string) => { x: number; y: number };
  onTake?: (id: string) => void;
}

export const useCloudDrag = (options: CloudDragOptions) => {
  const live = ref<{ id: string; x: number; y: number } | null>(null);
  const lastId = ref<string | null>(null);
  let start: { id: string; px: number; py: number; ox: number; oy: number; el: HTMLElement } | null = null;
  let dragging = false;
  let frame = 0;
  let pending = { x: 0, y: 0 };
  let trail: Array<{ t: number; x: number; y: number }> = [];

  const offsetOf = (id: string) => offsets.get(id) ?? { x: 0, y: 0 };

  const write = () => {
    frame = 0;
    if (!start) return;
    const vx = trail.length > 1 ? (trail[trail.length - 1]!.x - trail[0]!.x) / Math.max(1, trail[trail.length - 1]!.t - trail[0]!.t) : 0;
    start.el.style.translate = `${pending.x}px ${pending.y}px`;
    start.el.style.rotate = `${Math.max(-MAX_TILT, Math.min(MAX_TILT, vx * 6))}deg`;
    live.value = { id: start.id, x: pending.x, y: pending.y };
  };

  const onMove = (event: PointerEvent) => {
    if (!start) return;
    const dx = event.clientX - start.px;
    const dy = event.clientY - start.py;
    if (!dragging && Math.hypot(dx, dy) < START_PX) return;
    const now = performance.now();
    trail = [...trail.filter((p) => now - p.t < 90), { t: now, x: event.clientX, y: event.clientY }];
    pending = { x: start.ox + dx, y: start.oy + dy };
    if (!dragging) {
      dragging = true;
      start.el.classList.add('is-dragging');
      options.onTake?.(start.id);
      write();
      return;
    }
    if (!frame) frame = requestAnimationFrame(write);
  };

  const land = (id: string, el: HTMLElement, from: { x: number; y: number }, to: { x: number; y: number }) => {
    const dx = from.x - to.x;
    const dy = from.y - to.y;
    const home = reducedMotion() || (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5);
    if (home || !options.onLand) el.style.translate = `${to.x}px ${to.y}px`;
    if (home) return;
    if (options.onLand) { options.onLand(id, dx, dy); return; }
    const { easing, duration } = springCurve(SPRINGS.settle);
    el.animate([{ translate: `${from.x}px ${from.y}px` }, { translate: `${to.x}px ${to.y}px` }], { duration, easing });
  };

  const settleTilt = (el: HTMLElement) => {
    const tilt = el.style.rotate || '0deg';
    el.style.rotate = '';
    if (reducedMotion()) return;
    const { easing, duration } = springCurve(SPRINGS.settle);
    el.animate([{ rotate: tilt }, { rotate: '0deg' }], { duration, easing });
  };

  const end = (event: PointerEvent | null, cancelled: boolean) => {
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
    window.removeEventListener('pointercancel', onCancel);
    window.removeEventListener('keydown', onKey, true);
    cancelAnimationFrame(frame);
    frame = 0;
    const s = start;
    start = null;
    if (!s) return;
    if (!dragging) {
      if (!cancelled && event) options.onTap(s.id, s.el);
      return;
    }
    dragging = false;
    s.el.classList.remove('is-dragging');
    const vy = trail.length > 1 ? (trail[trail.length - 1]!.y - trail[0]!.y) / Math.max(1, trail[trail.length - 1]!.t - trail[0]!.t) : 0;
    const at = pending;
    if (!cancelled && options.canFlick(s.id) && vy < FLICK_VY) {
      live.value = null;
      const fly = reducedMotion() ? null : s.el.animate(
        [{ translate: `${at.x}px ${at.y}px`, rotate: s.el.style.rotate || '0deg', opacity: 1 },
          { translate: `${at.x + (trail.length ? 40 : 0)}px ${at.y - window.innerHeight}px`, rotate: '-18deg', opacity: 0 }],
        { duration: 420, easing: 'cubic-bezier(.3, .2, .6, 1)', fill: 'forwards' },
      );
      void (fly?.finished.catch(() => undefined) ?? Promise.resolve()).then(() => {
        offsets.delete(s.id);
        s.el.style.translate = '';
        s.el.style.rotate = '';
        options.onFlick(s.id);
        fly?.cancel();
      });
      return;
    }
    keepOnTop(s.id, s.el);
    offsets.delete(s.id);
    land(s.id, s.el, at, { x: 0, y: 0 });
    live.value = null;
    settleTilt(s.el);
  };
  const keepOnTop = (id: string, el: HTMLElement) => {
    lastId.value = id;
    el.classList.add('is-landing');
    window.setTimeout(() => el.classList.remove('is-landing'), 700);
  };
  const onUp = (event: PointerEvent) => end(event, false);
  const onCancel = () => end(null, true);
  const onKey = (event: KeyboardEvent) => {
    if (event.key !== 'Escape' || !dragging) return;
    event.preventDefault();
    event.stopPropagation();
    end(null, true);
  };

  const onDown = (event: PointerEvent, id: string) => {
    if (event.button !== 0 || start) return;
    const el = options.slotOf(id);
    if (!el) return;
    const o = offsetOf(id);
    const grabbed = options.grabAt?.(id) ?? { x: 0, y: 0 };
    start = { id, px: event.clientX, py: event.clientY, ox: o.x + grabbed.x, oy: o.y + grabbed.y, el };
    pending = { x: start.ox, y: start.oy };
    trail = [{ t: performance.now(), x: event.clientX, y: event.clientY }];
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    window.addEventListener('pointercancel', onCancel);
    window.addEventListener('keydown', onKey, true);
  };

  onBeforeUnmount(() => end(null, true));

  const resetOffsets = () => { offsets.clear(); lastId.value = null; };
  const moved = computed(() => [...offsets.values()].some((o) => Math.abs(o.x) + Math.abs(o.y) > 1));

  return { live, lastId, moved, offsetOf, onDown, resetOffsets, dragging: () => dragging };
};
