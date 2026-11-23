import { onBeforeUnmount, shallowRef } from 'vue';
import { stepCloud, type CloudBody, type CloudBounds } from '../../lib/aiTask/cloudPhysics';
import { reducedMotion } from '../../lib/motion/cards';

export interface CloudPhysicsOptions {
  slots: () => Array<{ id: string; x: number; y: number }>;
  offsetOf: (id: string) => { x: number; y: number };
  live: () => { id: string; x: number; y: number } | null;
  card: () => { width: number; height: number };
  bounds?: () => CloudBounds;
}

const NONE = Object.freeze({ x: 0, y: 0 });

export const useCloudPhysics = (options: CloudPhysicsOptions) => {
  const shifts = shallowRef<ReadonlyMap<string, { x: number; y: number }>>(new Map());
  const bodies = new Map<string, CloudBody>();
  let frame = 0;
  let last = 0;

  const sync = () => {
    const live = options.live();
    const seen = new Set<string>();
    for (const slot of options.slots()) {
      seen.add(slot.id);
      const o = options.offsetOf(slot.id);
      const rest = { x: slot.x + o.x, y: slot.y + o.y };
      const body = bodies.get(slot.id) ?? { id: slot.id, rest, d: { x: 0, y: 0 }, v: { x: 0, y: 0 } };
      body.rest = rest;
      body.pinned = live?.id === slot.id;
      if (body.pinned) {
        body.at = { x: slot.x + live!.x, y: slot.y + live!.y };
        body.d = { x: 0, y: 0 };
        body.v = { x: 0, y: 0 };
      } else body.at = undefined;
      bodies.set(slot.id, body);
    }
    for (const id of [...bodies.keys()]) if (!seen.has(id)) bodies.delete(id);
  };

  const tick = (now: number) => {
    frame = 0;
    sync();
    const calm = stepCloud([...bodies.values()], options.card(), last ? (now - last) / 1000 : 1 / 60, options.bounds?.());
    last = now;
    for (const b of bodies.values()) {
      if (b.bounce && Math.hypot(b.d.x, b.d.y) < 1.5 && Math.hypot(b.v.x, b.v.y) < 12) b.bounce = false;
    }
    shifts.value = new Map([...bodies.values()].map((b) => [b.id, b.pinned ? NONE : { x: b.d.x, y: b.d.y }]));
    if (calm && [...bodies.values()].every((b) => Math.abs(b.d.x) < 0.3 && Math.abs(b.d.y) < 0.3)) {
      shifts.value = new Map();
      for (const b of bodies.values()) { b.d = { x: 0, y: 0 }; b.v = { x: 0, y: 0 }; }
      last = 0;
      return;
    }
    if (calm && !options.live()) { last = 0; return; }
    frame = requestAnimationFrame(tick);
  };

  const kick = () => {
    if (reducedMotion() || frame || document.hidden) return;
    frame = requestAnimationFrame(tick);
  };
  const reset = () => {
    cancelAnimationFrame(frame);
    frame = 0;
    last = 0;
    bodies.clear();
    shifts.value = new Map();
  };
  const shiftOf = (id: string) => shifts.value.get(id) ?? NONE;
  const take = (id: string) => {
    const body = bodies.get(id);
    if (body) { body.d = { x: 0, y: 0 }; body.v = { x: 0, y: 0 }; body.bounce = false; }
    if (!shifts.value.has(id)) return;
    const next = new Map(shifts.value);
    next.delete(id);
    shifts.value = next;
  };
  const nudge = (id: string, dx: number, dy: number) => {
    sync();
    const body = bodies.get(id);
    if (!body) return;
    body.pinned = false;
    body.at = undefined;
    body.bounce = true;
    body.d = { x: body.d.x + dx, y: body.d.y + dy };
    shifts.value = new Map([...shifts.value, [id, { ...body.d }]]);
    kick();
  };

  onBeforeUnmount(() => cancelAnimationFrame(frame));
  return { shiftOf, kick, reset, nudge, take };
};
