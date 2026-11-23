import { lessMotion } from './lessMotion';
import { OPEN_EASE, paced } from './timing';
import { easeOf, lerpBox, sameBox, unionBox, type Box } from '../stage/flightRect';

export const STAGE_MS = paced(400);
export const SETTLE_EASE = 'cubic-bezier(.3, 1.22, .4, 1)';

export interface Snapshot {
  matrix: string; tx: number; ty: number; k: number; w: number; h: number; opacity: number; rect: Box | null;
  inline: string | null;
}

interface FrameFlight { a: Box; b: Box; start: number; duration: number; ease: (t: number) => number }
export interface FlySize { w: number; h: number; radius: number }
interface Flight { animations: Animation[]; frame?: FrameFlight }
const running = new WeakMap<HTMLElement, Flight>();
const FRAME_VARS = ['--fly-h', '--fl-t', '--fl-r', '--fl-b', '--fl-l', '--fl-ab'];

const bodyOf = (el: HTMLElement) => el.querySelector<HTMLElement>(':scope > .window-body');
const numbers = (text: string) => text.trim().split(/\s+/).map((v) => parseFloat(v) || 0);
const px = (n: number) => `${Math.round(n * 10) / 10}px`;
const radiusOf = (el: HTMLElement) => parseFloat(getComputedStyle(el).borderTopLeftRadius) || 0;

const clipBottom = (el: HTMLElement): number => {
  const body = bodyOf(el);
  const m = body ? getComputedStyle(body).clipPath.match(/^inset\(([^)]*?)(?: round|\))/) : null;
  if (!m) return 0;
  const edges = numbers(m[1]);
  return edges[2] ?? edges[0] ?? 0;
};

const project = (m: DOMMatrix, x: number, y: number) => {
  const p = m.transformPoint(new DOMPoint(x, y, 0, 1));
  return { x: p.x / (p.w || 1), y: p.y / (p.w || 1) };
};
const flatOrigin = (transform: string, w: number, h: number, allowScale = false): { x: number; y: number; k: number } | null => {
  let m: DOMMatrix;
  try { m = transform && transform !== 'none' ? new DOMMatrix(transform) : new DOMMatrix(); } catch { return null; }
  const o = project(m, 0, 0);
  const r = project(m, w, 0);
  const d = project(m, 0, h);
  const k = (r.x - o.x) / Math.max(1, w);
  const flat = Math.abs(r.y - o.y) < 0.5 && Math.abs(d.x - o.x) < 0.5 && Math.abs((d.y - o.y) - k * h) < 0.5 * Math.max(1, k);
  if (!flat || (!allowScale && Math.abs(k - 1) > 0.002)) return null;
  return { ...o, k };
};

const frameRectNow = (f: FrameFlight): Box => lerpBox(f.a, f.b, f.ease(Math.min(1, Math.max(0, (performance.now() - f.start) / f.duration))));

export const snapshotOf = (el: HTMLElement): Snapshot => {
  const style = getComputedStyle(el);
  const [tx = 0, ty = 0] = style.translate && style.translate !== 'none' ? numbers(style.translate) : [];
  const k = style.scale && style.scale !== 'none' ? parseFloat(style.scale) || 1 : 1;
  const flight = running.get(el);
  const h = Math.max(1, el.offsetHeight - clipBottom(el));
  let rect: Box | null = null;
  if (flight?.frame) rect = frameRectNow(flight.frame);
  else {
    const o = flatOrigin(style.transform, el.offsetWidth, h, true);
    if (o) rect = { x: tx + k * o.x, y: ty + k * o.y, w: k * o.k * el.offsetWidth, h: k * o.k * h };
  }
  const still = !flight && tx === 0 && ty === 0 && k === 1;
  const inline = still ? el.style.transform : flight ? null : withDrag(el.style.transform, tx, ty, k);
  return { matrix: style.transform === 'none' ? '' : style.transform, tx, ty, k, w: el.offsetWidth, h, opacity: Number(style.opacity), rect, inline };
};

const STRUCTURED = /^translate\(([-\d.]+)px, ([-\d.]+)px\) perspective\(([\d.]+)px\) rotateY\(([-\d.]+)deg\) translate\(0px, ([-\d.]+)px\) scale\(([\d.e-]+)\)$/;
const withDrag = (inline: string, tx: number, ty: number, k: number): string | null => {
  const a = inline.match(STRUCTURED);
  if (!a || parseFloat(a[4]) !== 0) return null;
  const r = (n: number) => Math.round(n * 100) / 100;
  return `translate(${r(tx + k * parseFloat(a[1]))}px, ${r(ty + k * parseFloat(a[2]))}px) perspective(${a[3]}px) rotateY(0deg) translate(0px, ${r(k * parseFloat(a[5]))}px) scale(${Math.round(k * parseFloat(a[6]) * 10000) / 10000})`;
};
const structuredStart = (inline: string | null, end: string, sx: number): string | null => {
  const a = inline?.match(STRUCTURED);
  if (!a || !STRUCTURED.test(end)) return null;
  const s = Math.round(parseFloat(a[6]) * sx * 10000) / 10000;
  return `translate(${a[1]}px, ${a[2]}px) perspective(${a[3]}px) rotateY(${a[4]}deg) translate(0px, ${a[5]}px) scale(${s})`;
};

export const stopFlight = (el: HTMLElement): void => {
  for (const animation of running.get(el)?.animations ?? []) animation.cancel();
  running.delete(el);
  for (const name of FRAME_VARS) el.style.removeProperty(name);
  el.classList.remove('is-flying');
};
export const isFlying = (el: HTMLElement): boolean => running.has(el);

export const dragTo = (el: HTMLElement, tx: number, ty: number, k = 1): void => {
  el.style.translate = `${Math.round(tx * 10) / 10}px ${Math.round(ty * 10) / 10}px`;
  el.style.scale = k === 1 ? '' : String(Math.round(k * 10000) / 10000);
};
export const clearDrag = (el: HTMLElement): void => {
  el.style.translate = '';
  el.style.scale = '';
};

const finishWith = (el: HTMLElement, flight: Flight, last: Animation) => {
  running.set(el, flight);
  const done = () => {
    if (running.get(el) !== flight) return;
    running.delete(el);
    for (const name of FRAME_VARS) el.style.removeProperty(name);
    el.classList.remove('is-flying');
  };
  last.addEventListener('finish', done, { once: true });
  last.finished.then(done, () => {});
};

type Launch = () => void;

const frameFlight = (el: HTMLElement, a: Box, b: Box, easing: string, fromOpacity: number, size: FlySize): Launch => {
  const body = bodyOf(el);
  const W = Math.max(1, size.w);
  const H = Math.max(1, size.h);
  const settle = easing !== OPEN_EASE;
  const duration = settle ? Math.round(STAGE_MS * 1.15) : STAGE_MS;
  const ease = easeOf(easing);
  const STEPS = 16;
  const boxes = Array.from({ length: STEPS + 1 }, (_, i) => {
    const r = lerpBox(a, b, ease(i / STEPS));
    return { x: r.x - b.x, y: r.y - b.y, w: r.w, h: r.h };
  });
  const flyH = Math.max(H, ...boxes.map((r) => r.h / (r.w / W)));
  if (flyH > H + 0.5) el.style.setProperty('--fly-h', px(flyH));
  const hull = unionBox([...boxes, { x: 0, y: 0, w: W, h: H }]);
  el.style.setProperty('--fl-t', px(hull.y));
  el.style.setProperty('--fl-l', px(hull.x));
  el.style.setProperty('--fl-r', px(W - (hull.x + hull.w)));
  el.style.setProperty('--fl-b', px(flyH - (hull.y + hull.h)));
  el.style.setProperty('--fl-ab', px(flyH - H));
  el.classList.add('is-flying');
  return () => {
    const R = size.radius;
    const timing: KeyframeAnimationOptions = { duration, easing: 'linear' };
    const animations: Animation[] = [];
    const offset = (i: number) => i / STEPS;
    animations.push(el.animate(boxes.map((r, i) => ({
      offset: offset(i),
      clipPath: `inset(${px(r.y - hull.y)} ${px(hull.x + hull.w - (r.x + r.w))} ${px(hull.y + hull.h - (r.y + r.h))} ${px(r.x - hull.x)} round ${px(R)})`,
    })), { ...timing, pseudoElement: '::before' }));
    animations.push(el.animate(boxes.map((r, i) => ({
      offset: offset(i),
      transform: `translate(${px(r.x)}, ${px(r.y)}) scale(${(r.w / W).toFixed(4)}, ${(r.h / H).toFixed(4)})`,
    })), { ...timing, pseudoElement: '::after' }));
    if (body) {
      animations.push(body.animate(boxes.map((r, i) => {
        const s = r.w / W;
        return {
          offset: offset(i),
          transform: `translate(${px(r.x)}, ${px(r.y)}) scale(${s.toFixed(4)})`,
          clipPath: `inset(0px 0px ${px(Math.max(0, flyH - r.h / s))} 0px round ${px(R)})`,
        };
      }), timing));
    }
    const opacity = Number(el.style.opacity || 1);
    if (Math.abs(fromOpacity - opacity) > 0.001) animations.push(el.animate([{ opacity: fromOpacity }, { opacity }], { duration, easing: OPEN_EASE }));
    const flight: Flight = { animations, frame: { a, b, start: performance.now(), duration, ease } };
    finishWith(el, flight, animations[0]);
  };
};

const prepare = (el: HTMLElement, from: Snapshot, easing: string, known?: FlySize): Launch | null => {
  stopFlight(el);
  clearDrag(el);
  if (lessMotion()) return null;
  const size = known ?? { w: el.offsetWidth, h: el.offsetHeight, radius: radiusOf(el) };
  const W = Math.max(1, size.w);
  const H = Math.max(1, size.h);
  const target = flatOrigin(el.style.transform, W, H);
  if (from.rect && target) {
    const b = { x: target.x, y: target.y, w: W, h: H };
    if (sameBox(from.rect, b) && Math.abs(from.opacity - Number(el.style.opacity || 1)) < 0.001) return null;
    return frameFlight(el, from.rect, b, easing, from.opacity, size);
  }
  const sx = from.w / W;
  const body = bodyOf(el);
  const animations: Animation[] = [];
  const settle = easing !== OPEN_EASE;
  const timing: KeyframeAnimationOptions = { duration: settle ? Math.round(STAGE_MS * 1.15) : STAGE_MS, easing };
  const plain: KeyframeAnimationOptions = { ...timing, easing: OPEN_EASE };
  const startH = from.h / sx;
  const tall = Math.max(H, startH);
  if (Math.abs(startH - H) > 0.5 && tall > H + 0.5) el.style.setProperty('--fly-h', px(tall));
  el.classList.add('is-flying');
  return () => {
    if (Math.abs(startH - H) > 0.5) {
      const R = size.radius;
      const inset = (bottom: number) => `inset(0px 0px ${px(Math.max(0, bottom))} 0px round ${px(R)})`;
      const clip = [{ clipPath: inset(tall - startH) }, { clipPath: inset(tall - H) }];
      if (body) animations.push(body.animate(clip, plain));
      animations.push(el.animate(clip, { ...plain, pseudoElement: '::before' }));
      animations.push(el.animate([{ transform: `scale(1, ${(startH / tall).toFixed(4)})` }, { transform: `scale(1, ${(H / tall).toFixed(4)})` }], { ...plain, pseudoElement: '::after' }));
    }
    const end = el.style.transform || 'none';
    const start = structuredStart(from.inline, end, sx) ?? `translate(${from.tx}px, ${from.ty}px) scale(${from.k}) ${from.matrix} scale(${sx})`;
    const opacity = Number(el.style.opacity || 1);
    const animation = el.animate([{ transform: start }, { transform: end }], timing);
    animations.push(animation);
    if (Math.abs(from.opacity - opacity) > 0.001) animations.push(el.animate([{ opacity: from.opacity }, { opacity }], plain));
    finishWith(el, { animations }, animation);
  };
};

export const flyFrom = (el: HTMLElement, from: Snapshot, easing = OPEN_EASE, known?: FlySize): void => {
  prepare(el, from, easing, known)?.();
};

export const flyAll = (items: ReadonlyArray<{ el: HTMLElement; from: Snapshot; easing?: string; size?: FlySize }>): void => {
  const launches = items.map((item) => prepare(item.el, item.from, item.easing ?? OPEN_EASE, item.size));
  for (const launch of launches) launch?.();
};
