import { CLOSE_EASE, paced } from '../timing';
import { animateFromNow } from './reversible';
import { centroid, poseTransform, restOf, stackPose, type Point, type Rest } from './pose';
import { reducedMotion, settled, SPRINGS, springCurve } from './spring';

const STAGGER_MS = paced(34);
const DRAW_MS = paced(120);
const STACK_STAGGER_MS = paced(26);
const STACK_MS = paced(320);
const STACK_EASE = 'cubic-bezier(.25, .8, .25, 1)';
const RETURN_MS = paced(440);
export const CARD_RETURN_MS = RETURN_MS;
const CROSSFADE_MS = paced(100);
const FADE_MS = paced(160);

export interface Landing {
  rect: DOMRect;
  kind: 'button' | 'pile';
  angle?: number;
  width?: number;
}

const centerOf = (rect: DOMRect): Point => ({ x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 });

const landingScale = (landing: Landing, cardWidth: number): number => {
  const w = Math.max(cardWidth, 1);
  if (landing.kind === 'pile') return (landing.width ?? landing.rect.width) / w;
  return Math.max(0.06, Math.min(landing.rect.width / w, landing.rect.height / (w * 1.4), 0.5));
};

export interface DealOptions {
  fromStack?: boolean;
  track?: (animations: Animation[]) => void;
}

export const dealCards = async (cards: HTMLElement[], origin: Landing | null, options: DealOptions = {}): Promise<void> => {
  if (!cards.length) return;
  if (reducedMotion()) {
    await settled(cards.map((card) => card.animate([{ opacity: 0 }, { opacity: 1 }], { duration: FADE_MS, fill: 'backwards' })));
    return;
  }
  const rests = cards.map(restOf);
  const middle = centroid(rests);
  const source: Landing = origin ?? { rect: new DOMRect(middle.x - 40, window.innerHeight - 60, 80, 110), kind: 'button' };
  const from = centerOf(source.rect);
  const n = cards.length;
  const { easing, duration } = springCurve(SPRINGS.deal);
  const startScale = landingScale(source, rests[0]!.width);
  const drawn: Point = options.fromStack ? from : { x: from.x + (middle.x - from.x) * 0.34, y: from.y + (middle.y - from.y) * 0.34 };
  const drawnScale = options.fromStack ? startScale : Math.max(startScale, Math.min(0.62, startScale * 2.4 + 0.18));
  const baseAngle = source.kind === 'pile' ? source.angle ?? 0 : 0;
  const start = (rest: Rest, i: number) => poseTransform(rest, stackPose(from, i, n, startScale, baseAngle));
  const pulled = (rest: Rest, i: number) => poseTransform(rest, stackPose(drawn, i, n, drawnScale, baseAngle));
  const fan = (card: HTMLElement, i: number, rest: Rest) => card.animate(
    [{ transform: pulled(rest, i), opacity: 1 }, { transform: 'none', opacity: 1 }],
    { duration, delay: Math.abs(i - (n - 1) / 2) * STAGGER_MS, easing, fill: 'backwards' },
  );
  const track = options.track ?? (() => undefined);
  if (options.fromStack) {
    const fans = cards.map((card, i) => fan(card, i, rests[i]!));
    track(fans);
    await settled(fans);
    return;
  }
  const draws = cards.map((card, i) => card.animate(
    [{ transform: start(rests[i]!, i), opacity: 0 }, { opacity: 1, offset: 0.45 }, { transform: pulled(rests[i]!, i), opacity: 1 }],
    { duration: DRAW_MS, easing: 'cubic-bezier(.2, .7, .3, 1)', fill: 'both' },
  ));
  track(draws);
  await settled(draws);
  if (draws.some((a) => a.playState !== 'finished')) return;
  const fans = cards.map((card, i) => fan(card, i, rests[i]!));
  for (const a of draws) a.cancel();
  track(fans);
  await settled(fans);
};

export const stackCards = async (cards: HTMLElement[], center?: Point, angle = 0): Promise<Animation[]> => {
  if (!cards.length) return [];
  if (reducedMotion()) return [];
  const rests = cards.map(restOf);
  const at = center ?? centroid(rests);
  const n = cards.length;
  const mid = (n - 1) / 2;
  const animations = cards.map((card, i) => animateFromNow(
    card,
    { transform: poseTransform(rests[i]!, stackPose(at, i, n, 1, angle)), opacity: 1 },
    { duration: STACK_MS, delay: (mid - Math.abs(i - mid)) * STACK_STAGGER_MS, easing: STACK_EASE, fill: 'forwards' },
  ));
  await Promise.race([settled(animations), new Promise<void>((resolve) => window.setTimeout(resolve, stackHandoffMs(n)))]);
  return animations;
};

export const stackHandoffMs = (n: number): number => (reducedMotion() ? 0 : Math.max(0, (n - 1) / 2) * STACK_STAGGER_MS + STACK_MS * 0.72);
export const stackAndReturnMs = (n: number): number => stackHandoffMs(n) + RETURN_MS + Math.max(0, n - 1) * 6;

export const returnStack = async (cards: HTMLElement[], landing: Landing | null): Promise<Animation[]> => {
  if (!cards.length) return [];
  if (reducedMotion() || !landing) {
    const fades = cards.map((card) => animateFromNow(card, { opacity: 0 }, { duration: FADE_MS, fill: 'forwards' }));
    await settled(fades);
    return fades;
  }
  const rests = cards.map(restOf);
  const n = cards.length;
  const to = centerOf(landing.rect);
  const scale = landingScale(landing, rests[0]!.width);
  const baseAngle = landing.kind === 'pile' ? landing.angle ?? 0 : 0;
  const now = centroid(rests.map((rest, i) => ({ ...rest, center: currentCenter(cards[i]!) ?? rest.center })));
  const lift = Math.min(90, 24 + Math.hypot(to.x - now.x, to.y - now.y) * 0.12);
  const animations = cards.map((card, i) => {
    const rest = rests[i]!;
    const end = poseTransform(rest, landing.kind === 'pile'
      ? { center: to, angle: baseAngle, scale }
      : stackPose(to, i, n, scale));
    const midPose = stackPose({ x: (now.x + to.x) / 2, y: (now.y + to.y) / 2 - lift }, i, n, (1 + scale) / 2, baseAngle / 2);
    const keys: Keyframe[] = [
      { transform: poseTransform(rest, midPose), opacity: 1, offset: 0.5 },
      ...(landing.kind === 'button' ? [{ opacity: 1, offset: 0.7 }] : []),
      { transform: end, opacity: landing.kind === 'button' ? 0 : 1 },
    ];
    return animateFromNow(card, keys, { duration: RETURN_MS, delay: (n - 1 - i) * 6, easing: CLOSE_EASE, fill: 'forwards' });
  });
  await settled(animations);
  if (landing.kind === 'pile' && animations.every((a) => a.playState === 'finished')) {
    const fades = cards.map((card) => animateFromNow(card, { opacity: 0 }, { duration: CROSSFADE_MS, easing: 'ease-out', fill: 'forwards' }));
    await settled(fades);
    return fades;
  }
  return animations;
};

const currentCenter = (card: HTMLElement): Point | null => {
  const rect = card.getBoundingClientRect();
  return rect.width ? centerOf(rect) : null;
};

export const stackAndReturn = async (cards: HTMLElement[], landing: Landing | null, center?: Point): Promise<void> => {
  await stackCards(cards, center);
  await returnStack(cards, landing);
};

export const fanBack = async (cards: HTMLElement[]): Promise<void> => {
  const { easing, duration } = springCurve(SPRINGS.settle);
  await settled(cards.map((card) => animateFromNow(card, { transform: 'none', opacity: 1 }, { duration, easing })));
};
