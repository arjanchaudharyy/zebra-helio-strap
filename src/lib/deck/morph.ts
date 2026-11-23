export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

export const unscaledBox = (box: Box, origin: { x: number; y: number }, scale: number): Box => {
  const k = scale > 0 ? scale : 1;
  return {
    left: origin.x + (box.left - origin.x) / k,
    top: origin.y + (box.top - origin.y) / k,
    width: box.width / k,
    height: box.height / k,
  };
};

export interface Point {
  x: number;
  y: number;
}

export const centerOf = (box: Box): Point => ({ x: box.left + box.width / 2, y: box.top + box.height / 2 });

export function staggerOrder(ids: string[], center: string | null, reverse = false): Map<string, number> {
  const middle = Math.max(0, center ? ids.indexOf(center) : 0);
  const n = ids.length;
  const distance = (index: number) => {
    const d = Math.abs(index - middle);
    return Math.min(d, n - d);
  };
  const sorted = ids.map((id, index) => ({ id, d: distance(index), index }))
    .sort((a, b) => a.d - b.d || a.index - b.index);
  const order = reverse ? sorted.reverse() : sorted;
  return new Map(order.map((entry, rank) => [entry.id, rank]));
}

export const peekBox = (box: Box, nextTop: number | null): Box => {
  if (nextTop === null || nextTop <= box.top || nextTop >= box.top + box.height) return box;
  return { ...box, height: nextTop - box.top };
};

export const SHADOW_BLEED = 100;

export const shownRect = (card: Box, viewportHeight: number, bleed = SHADOW_BLEED): Box => {
  const top = card.top < 0 ? 0 : card.top - bleed;
  const bottom = card.top + card.height > viewportHeight ? viewportHeight : card.top + card.height + bleed;
  return { left: card.left - bleed, top, width: card.width + bleed * 2, height: Math.max(1, bottom - top) };
};

const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
const px = (value: number) => `${Number(value.toFixed(2))}px`;

export interface CardPose {
  rect: Box;
  anchor: Point;
}

export const cardFrame = (pose: CardPose, card: Box, radius: number): Keyframe => {
  const { rect, anchor } = pose;
  const top = rect.top - anchor.y;
  const left = rect.left - anchor.x;
  const right = anchor.x + card.width - (rect.left + rect.width);
  const bottom = anchor.y + card.height - (rect.top + rect.height);
  return {
    transform: `translate(${px(anchor.x - card.left)}, ${px(anchor.y - card.top)})`,
    clipPath: `inset(${px(top)} ${px(right)} ${px(bottom)} ${px(left)} round ${px(radius)})`,
  };
};

export const containedPose = (pose: CardPose, card: Box): CardPose => {
  const { rect, anchor } = pose;
  const left = Math.max(rect.left, anchor.x);
  const top = Math.max(rect.top, anchor.y);
  const right = Math.min(rect.left + rect.width, anchor.x + card.width);
  const bottom = Math.min(rect.top + rect.height, anchor.y + card.height);
  return { anchor, rect: { left, top, width: Math.max(1, right - left), height: Math.max(1, bottom - top) } };
};

const bleeds = (pose: CardPose, card: Box) =>
  pose.rect.left < pose.anchor.x - 0.01 && pose.rect.left + pose.rect.width > pose.anchor.x + card.width + 0.01;

export const BLEED_EDGE = 0.06;

export const morphFrames = (from: CardPose, to: CardPose, card: Box, radius: number): Keyframe[] => {
  const t = 0.5;
  const tv = 0.64;
  const mid: CardPose = {
    rect: {
      left: lerp(from.rect.left, to.rect.left, t),
      width: lerp(from.rect.width, to.rect.width, t),
      top: lerp(from.rect.top, to.rect.top, tv),
      height: lerp(from.rect.height, to.rect.height, tv),
    },
    anchor: { x: lerp(from.anchor.x, to.anchor.x, t), y: lerp(from.anchor.y, to.anchor.y, tv) },
  };
  const frame = (pose: CardPose, offset: number): Keyframe => ({ ...cardFrame(pose, card, radius), offset });
  const a = containedPose(from, card);
  const b = containedPose(to, card);
  const out: Keyframe[] = [];
  if (bleeds(from, card)) out.push(frame(from, 0), frame(a, BLEED_EDGE));
  else out.push(frame(a, 0));
  out.push(frame(containedPose(mid, card), 0.5));
  if (bleeds(to, card)) out.push(frame(b, 1 - BLEED_EDGE), frame(to, 1));
  else out.push(frame(b, 1));
  return out;
};

export const flightOffset = (center: Point, origin: Point, visual: Point, k: number): Point => ({
  x: center.x - origin.x - k * (visual.x - origin.x),
  y: center.y - origin.y - k * (visual.y - origin.y),
});
