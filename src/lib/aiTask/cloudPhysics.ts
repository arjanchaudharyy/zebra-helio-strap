export interface CloudBody {
  id: string;
  rest: { x: number; y: number };
  d: { x: number; y: number };
  v: { x: number; y: number };
  pinned?: boolean;
  at?: { x: number; y: number };
  bounce?: boolean;
}

export interface CloudBounds { x: number; y: number; width: number; height: number }

export const SPRING = 150;
export const DAMPING = 24;
export const BOUNCE_DAMPING = 11;
export const PUSH = 120000;
const WALL = 900;
const REACH = { x: 1.14, y: 1.1 };
export const DRAG_REACH = { x: 1.62, y: 1.5 };
const P = 4;
const MAX_DT = 1 / 30;

export const positionOf = (body: CloudBody) => (body.pinned && body.at ? body.at : { x: body.rest.x + body.d.x, y: body.rest.y + body.d.y });

export const separation = (dx: number, dy: number, rx: number, ry: number, tie: number) => {
  let nx = dx / rx;
  let ny = dy / ry;
  if (Math.abs(nx) < 1e-6 && Math.abs(ny) < 1e-6) { nx = tie * 1e-3; ny = 0; }
  const r = Math.max(1e-3, (Math.abs(nx) ** P + Math.abs(ny) ** P) ** (1 / P));
  const gx = Math.sign(nx) * Math.abs(nx) ** (P - 1);
  const gy = Math.sign(ny) * Math.abs(ny) ** (P - 1);
  const gl = Math.hypot(gx, gy) || 1;
  const rl = Math.hypot(nx, ny) || 1;
  const ux = gx / gl + nx / rl;
  const uy = gy / gl + ny / rl;
  const ul = Math.hypot(ux, uy) || 1;
  return { r, ux: ux / ul, uy: uy / ul };
};

export const stepCloud = (bodies: CloudBody[], card: { width: number; height: number }, dtSeconds: number, bounds?: CloudBounds): boolean => {
  const dt = Math.min(MAX_DT, Math.max(0, dtSeconds));
  const fx = new Map<string, number>();
  const fy = new Map<string, number>();
  for (const b of bodies) {
    const damping = b.bounce ? BOUNCE_DAMPING : DAMPING;
    fx.set(b.id, -SPRING * b.d.x - damping * b.v.x);
    fy.set(b.id, -SPRING * b.d.y - damping * b.v.y);
  }
  for (let i = 0; i < bodies.length; i += 1) {
    for (let j = i + 1; j < bodies.length; j += 1) {
      const a = bodies[i]!;
      const b = bodies[j]!;
      const reach = a.pinned || b.pinned ? DRAG_REACH : REACH;
      const pa = positionOf(a);
      const pb = positionOf(b);
      const { r, ux, uy } = separation(pa.x - pb.x, pa.y - pb.y, card.width * reach.x, card.height * reach.y, a.id < b.id ? -1 : 1);
      if (r >= 1) continue;
      const push = PUSH * (1 - r);
      const shareA = a.pinned ? 0 : b.pinned ? 1 : 0.5;
      const shareB = b.pinned ? 0 : a.pinned ? 1 : 0.5;
      fx.set(a.id, fx.get(a.id)! + ux * push * shareA);
      fy.set(a.id, fy.get(a.id)! + uy * push * shareA);
      fx.set(b.id, fx.get(b.id)! - ux * push * shareB);
      fy.set(b.id, fy.get(b.id)! - uy * push * shareB);
    }
  }
  if (bounds) {
    for (const b of bodies) {
      if (b.pinned || b.bounce) continue;
      const p = positionOf(b);
      const right = bounds.x + Math.max(0, bounds.width - card.width);
      const bottom = bounds.y + Math.max(0, bounds.height - card.height);
      if (p.x < bounds.x) fx.set(b.id, fx.get(b.id)! + WALL * (bounds.x - p.x));
      else if (p.x > right) fx.set(b.id, fx.get(b.id)! - WALL * (p.x - right));
      if (p.y < bounds.y) fy.set(b.id, fy.get(b.id)! + WALL * (bounds.y - p.y));
      else if (p.y > bottom) fy.set(b.id, fy.get(b.id)! - WALL * (p.y - bottom));
    }
  }
  let calm = true;
  for (const b of bodies) {
    if (b.pinned) { b.v = { x: 0, y: 0 }; continue; }
    b.v = { x: b.v.x + fx.get(b.id)! * dt, y: b.v.y + fy.get(b.id)! * dt };
    b.d = { x: b.d.x + b.v.x * dt, y: b.d.y + b.v.y * dt };
    if (Math.abs(b.v.x) > 2 || Math.abs(b.v.y) > 2 || Math.abs(fx.get(b.id)!) > 40 || Math.abs(fy.get(b.id)!) > 40) calm = false;
  }
  return calm && !bodies.some((b) => b.pinned);
};
