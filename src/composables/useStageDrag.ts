import { ref, shallowRef, type Ref } from 'vue';
import { dropAt, passedSlop, resizeOutcome, type Drop, type Handle, type Resize, type ResizeContext } from '../lib/stage/gestures';
import { splitsOf, STRIP_W, THUMB_W, type Layout, type Placement } from '../lib/stage/layout';
import * as S from '../lib/stage/state';
import { dragTo, flyFrom, SETTLE_EASE, snapshotOf, stopFlight } from '../lib/motion/stageFlight';
import { lessMotion } from '../lib/motion/lessMotion';
import type { WorkoutStage } from './useWorkoutStage';

interface Drag {
  id: string; kind: 'move' | 'resize'; handle: Handle; x: number; y: number; el: HTMLElement;
  p: Placement;
  base: Layout;
  moving: boolean;
  gx: number; gy: number;
  dx: number; dy: number;
  s: number; target: number;
  frame: number;
  ctx: ResizeContext | null;
  outcome: Resize;
}

export const useStageDrag = (o: {
  stage: WorkoutStage;
  layout: Ref<Layout>;
  canvas: Ref<HTMLElement | null>;
  size: { width: number; height: number };
  element: (id: string) => HTMLElement | undefined;
  solveWith: (state: S.StageState) => Layout;
  onStart: () => void;
}) => {
  const drag = shallowRef<Drag | null>(null);
  const justDragged = ref(false);
  let instant = false;
  const releasing = new Set<string>();

  const point = (e: { clientX: number; clientY: number }) => {
    const box = o.canvas.value?.getBoundingClientRect();
    return { x: e.clientX - (box?.left ?? 0), y: e.clientY - (box?.top ?? 0) };
  };
  const dropOf = (d: Drag, e: { clientX: number; clientY: number }) => dropAt(
    point(e),
    { width: o.size.width, height: Math.max(o.size.height, d.base.canvasH), stageX: d.base.stageX, narrow: d.base.narrow },
    o.stage.state.value.stage.ids.map((id) => ({ id, ...(d.base.placements.get(id) ?? { x: 0, y: 0, w: 0, h: 0 }) })),
    d.id,
  );

  const outcomeOf = (id: string, drop: Drop): S.StageState | null => {
    const state = o.stage.state.value;
    const ids = state.stage.ids;
    if (ids.includes(id)) {
      if (drop.kind === 'strip') return S.removeFromStage(state, id, o.stage.wide);
      if (drop.kind === 'stage' && drop.onto) return S.swapOnStage(state, id, drop.onto);
      return null;
    }
    if (drop.kind === 'stage') return S.addToStage(state, id, drop.onto ? ids.indexOf(drop.onto) : drop.index, o.stage.wide);
    return null;
  };
  const preview = (next: S.StageState | null, continuous = false) => {
    instant = continuous;
    const same = next && JSON.stringify(next) === JSON.stringify(o.stage.state.value);
    o.stage.draft.value = same ? null : next;
  };

  const liftOf = (d: Drag, drop: Drop) => {
    const visual = d.p.w * d.p.scale;
    if (drop.kind === 'strip') return Math.min(1, (THUMB_W * 1.1) / visual);
    if (d.p.mode === 'strip') return 1.15;
    return Math.min(1.03, Math.max(0.45, 460 / visual));
  };
  const paint = (d: Drag) => {
    dragTo(d.el, (1 - d.s) * d.gx + d.dx, (1 - d.s) * d.gy + d.dy, d.s);
  };
  const tick = () => {
    const d = drag.value;
    if (!d || d.kind !== 'move') return;
    const gap = d.target - d.s;
    d.s = Math.abs(gap) < 0.002 ? d.target : d.s + gap * 0.2;
    paint(d);
    d.frame = requestAnimationFrame(tick);
  };

  let dropKey = '';
  const updateDrop = (d: Drag, e: PointerEvent) => {
    const drop = dropOf(d, e);
    d.target = lessMotion() ? 1 : liftOf(d, drop);
    const key = JSON.stringify(drop);
    if (key === dropKey) return;
    dropKey = key;
    preview(outcomeOf(d.id, drop));
  };

  const contextOf = (id: string, p: Placement): ResizeContext | null => {
    if (!p.sides) return null;
    const group = o.stage.state.value.stage;
    const { ids, snap } = group;
    const rects = ids.map((other) => o.layout.value.placements.get(other)).filter((q): q is Placement => !!q && q.mode === 'stage');
    const left = Math.min(...rects.map((q) => q.x));
    const right = Math.max(...rects.map((q) => q.x + q.w));
    return {
      count: ids.length, index: ids.indexOf(id), snap, id, sides: p.sides,
      avail: o.size.width - STRIP_W, width: o.size.width, groupW: right - left,
      startW: p.w, startH: p.h, maxH: Math.max(p.h, o.size.height), splits: splitsOf(group),
    };
  };
  const previewResize = (d: Drag) => {
    if (!d.ctx) return;
    const before = d.outcome.kind;
    d.outcome = resizeOutcome(d.ctx, d.handle, d.dx, d.dy);
    const state = o.stage.state.value;
    d.el.classList.toggle('resize-tuck', d.outcome.kind === 'tuck');
    if (d.outcome.kind === 'size') preview(S.setSize(state, d.outcome.patch), before === 'size' || before === 'none');
    else if (d.outcome.kind === 'full') preview(S.fullscreen(state, d.id));
    else if (d.outcome.kind === 'none') preview(null, before !== 'full');
  };

  const move = (e: PointerEvent) => {
    const d = drag.value;
    if (!d) return;
    d.dx = e.clientX - d.x;
    d.dy = e.clientY - d.y;
    if (!d.moving) {
      if (!passedSlop(d.dx, d.dy)) return;
      d.moving = true;
      stopFlight(d.el);
      d.el.classList.add(d.kind === 'move' ? 'is-dragging' : 'resizing');
      o.onStart();
      if (d.kind === 'move') d.frame = requestAnimationFrame(tick);
    }
    if (d.kind === 'move') {
      updateDrop(d, e);
      if (lessMotion()) { d.s = d.target; paint(d); }
    } else previewResize(d);
  };

  const finish = (e: PointerEvent) => {
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', finish);
    window.removeEventListener('pointercancel', finish);
    const d = drag.value;
    drag.value = null;
    dropKey = '';
    if (!d?.moving) { o.stage.draft.value = null; return; }
    cancelAnimationFrame(d.frame);
    d.el.classList.remove('is-dragging', 'resizing', 'resize-tuck');
    justDragged.value = true;
    setTimeout(() => { justDragged.value = false; }, 0);
    const { stage } = o;
    instant = d.kind === 'resize' && d.outcome.kind === 'size';
    if (e.type === 'pointercancel') stage.draft.value = null;
    else if (d.kind === 'move') {
      releasing.add(d.id);
      const drop = dropOf(d, e);
      const ids = stage.state.value.stage.ids;
      stage.draft.value = null;
      if (ids.includes(d.id)) {
        if (drop.kind === 'strip') stage.removeFromStage(d.id);
        else if (drop.kind === 'stage' && drop.onto) stage.swapOnStage(d.id, drop.onto);
      } else if (drop.kind === 'stage') {
        stage.addToStage(d.id, drop.onto ? ids.indexOf(drop.onto) : drop.index);
      }
      setTimeout(() => releasing.clear(), 0);
    } else if (d.outcome.kind === 'tuck') {
      stage.draft.value = null;
      stage.removeFromStage(d.id);
    } else stage.commitDraft();
    setTimeout(() => {
      instant = false;
      if (d.el.style.translate || d.el.style.scale) flyFrom(d.el, snapshotOf(d.el), d.kind === 'move' ? SETTLE_EASE : undefined);
    }, 0);
  };

  const grab = (id: string, e: PointerEvent, kind: 'move' | 'resize', handle: Handle = 'br') => {
    if (e.button !== 0 || drag.value) return;
    const el = o.element(id);
    const p = o.layout.value.placements.get(id);
    if (!el || !p || o.layout.value.classic) return;
    const at = point(e);
    drag.value = {
      id, kind, handle, x: e.clientX, y: e.clientY, el, p, base: o.layout.value, moving: false, gx: at.x, gy: at.y, dx: 0, dy: 0,
      s: 1, target: 1, frame: 0, ctx: kind === 'resize' ? contextOf(id, p) : null, outcome: { kind: 'none' },
    };
    if (kind === 'resize') e.preventDefault();
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', finish);
    window.addEventListener('pointercancel', finish);
  };

  const pinned = (): { id: string; p: Placement } | null => {
    const d = drag.value;
    return d && d.kind === 'move' && d.moving ? { id: d.id, p: d.p } : null;
  };

  return { grab, justDragged, releasing, dragging: drag, pinned, isInstant: () => instant };
};
