export interface Point { x: number; y: number }

export interface Rest { center: Point; angle: number; width: number }

export interface Pose { center: Point; angle: number; scale: number }

export const centerOfRect = (rect: DOMRect): Point => ({ x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 });

export const angleOf = (el: Element | null | undefined): number => {
  if (!el) return 0;
  const transform = getComputedStyle(el).transform;
  if (!transform || transform === 'none') return 0;
  const m = /matrix\(([^)]+)\)/.exec(transform);
  if (!m) return 0;
  const [a, b] = m[1]!.split(',').map(Number);
  return (Math.atan2(b ?? 0, a ?? 1) * 180) / Math.PI;
};

export const restOf = (card: HTMLElement): Rest => {
  const slot = card.parentElement?.classList.contains('pslot') ? card.parentElement : null;
  const box = (slot ?? card).getBoundingClientRect();
  return { center: centerOfRect(box), angle: slot ? angleOf(slot) : 0, width: card.offsetWidth || box.width };
};

export const toLocal = (dx: number, dy: number, deg: number): Point => {
  const r = (-deg * Math.PI) / 180;
  return { x: dx * Math.cos(r) - dy * Math.sin(r), y: dx * Math.sin(r) + dy * Math.cos(r) };
};

export const poseTransform = (rest: Rest, pose: Pose): string => {
  const local = toLocal(pose.center.x - rest.center.x, pose.center.y - rest.center.y, rest.angle);
  return `translate(${local.x.toFixed(1)}px, ${local.y.toFixed(1)}px) rotate(${(pose.angle - rest.angle).toFixed(2)}deg) scale(${pose.scale.toFixed(3)})`;
};

export const stackPose = (center: Point, i: number, n: number, scale = 1, baseAngle = 0): Pose => {
  const side = i % 2 === 0 ? -1 : 1;
  const tilt = side * (1 + ((i * 7) % 4));
  return {
    center: { x: center.x + (i - (n - 1) / 2) * 1.5 * scale, y: center.y - i * 1.5 * scale },
    angle: baseAngle + tilt * (i === n - 1 ? 0.4 : 1),
    scale,
  };
};

export const centroid = (rests: Rest[]): Point => {
  if (!rests.length) return { x: window.innerWidth / 2, y: window.innerHeight / 2 };
  const x = rests.reduce((sum, r) => sum + r.center.x, 0) / rests.length;
  const y = rests.reduce((sum, r) => sum + r.center.y, 0) / rests.length;
  return { x, y };
};
