
export interface WindowRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

const r2 = (value: number) => Number(value.toFixed(2));
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

export function windowInset(rect: WindowRect, frame: WindowRect, radius: number, bleed = false): string {
  const clamp = (value: number) => (bleed ? value : Math.max(0, value));
  const top = clamp(rect.top - frame.top);
  const left = clamp(rect.left - frame.left);
  const right = clamp(frame.left + frame.width - (rect.left + rect.width));
  const bottom = clamp(frame.top + frame.height - (rect.top + rect.height));
  return `inset(${r2(top)}px ${r2(right)}px ${r2(bottom)}px ${r2(left)}px round ${r2(radius)}px)`;
}

export function steadyRadius(from: WindowRect, fromRadius: number, to: WindowRect, toRadius: number): number {
  if (fromRadius <= 0) return Math.max(0, toRadius);
  if (toRadius <= 0) return fromRadius;
  return from.width * from.height <= to.width * to.height ? fromRadius : toRadius;
}

export function visibleWindow(el: HTMLElement): WindowRect {
  const box = el.getBoundingClientRect();
  const clip = getComputedStyle(el).clipPath.match(/^inset\((.*?) round/);
  const edges = clip?.[1].split(/\s+/).map(Number.parseFloat);
  if (!edges?.length || edges.some(value => !Number.isFinite(value))) return box;
  const top = edges[0], right = edges[1] ?? top, bottom = edges[2] ?? top, left = edges[3] ?? right;
  return { left: box.left + left, top: box.top + top, width: box.width - left - right, height: box.height - top - bottom };
}

export function belowLine(rect: WindowRect, line: number, minH = 24): WindowRect {
  if (rect.top >= line) return rect;
  const bottom = Math.max(rect.top + rect.height, line + minH);
  return { left: rect.left, top: line, width: rect.width, height: bottom - line };
}

export function bleedRect(rect: WindowRect, radius: number): WindowRect {
  return { left: rect.left - radius, top: rect.top - radius, width: rect.width + radius * 2, height: rect.height + radius * 2 };
}

export function arcMidpoint(from: WindowRect, to: WindowRect, t = 0.5, lead = 0.14): WindowRect {
  const tv = Math.min(1, t + lead);
  return {
    left: lerp(from.left, to.left, t),
    width: lerp(from.width, to.width, t),
    top: lerp(from.top, to.top, tv),
    height: lerp(from.height, to.height, tv),
  };
}

export const WINDOW_OFFSETS = [0, 0.5, 0.9, 1] as const;

export { cardReplica } from './replica';

export interface WindowPose {
  rect: WindowRect;
  anchor: { x: number; y: number };
}

const PROGRESS: ReadonlyArray<readonly [number, number]> = [[0, 0], [0.5, 0.64], [0.94, 0.96], [1, 1]];

export function windowPoses(from: WindowPose, to: WindowPose): WindowPose[] {
  return PROGRESS.map(([px, py]) => ({
    rect: {
      left: lerp(from.rect.left, to.rect.left, px),
      width: lerp(from.rect.width, to.rect.width, px),
      top: lerp(from.rect.top, to.rect.top, py),
      height: lerp(from.rect.height, to.rect.height, py),
    },
    anchor: { x: lerp(from.anchor.x, to.anchor.x, px), y: lerp(from.anchor.y, to.anchor.y, py) },
  }));
}

export function pageBackdrop(page: HTMLElement, box: WindowRect, view: WindowRect, glow: WindowRect, radius: number): HTMLElement {
  const bleed = bleedRect(view, radius);
  const left = Math.min(0, bleed.left - box.left);
  const top = Math.min(0, bleed.top - box.top);
  const right = Math.max(box.width, bleed.left + bleed.width - box.left);
  const bottom = Math.max(box.height, bleed.top + bleed.height - box.top);
  const backdrop = document.createElement('div');
  backdrop.setAttribute('aria-hidden', 'true');
  backdrop.dataset.morphLayer = '';
  Object.assign(backdrop.style, {
    position: 'absolute',
    left: `${r2(left)}px`,
    top: `${r2(top)}px`,
    width: `${r2(right - left)}px`,
    height: `${r2(bottom - top)}px`,
    zIndex: '-1',
    pointerEvents: 'none',
    background: 'var(--mat-card-solid)',
  });
  const canvas = document.createElement('div');
  Object.assign(canvas.style, { position: 'absolute', inset: '0', overflow: 'hidden', background: 'var(--canvas)' });
  const ambient = document.createElement('div');
  Object.assign(ambient.style, {
    position: 'absolute',
    left: `${r2(glow.left - box.left - left)}px`,
    top: `${r2(glow.top - box.top - top)}px`,
    width: `${r2(glow.width)}px`,
    height: `${r2(glow.height)}px`,
    background: 'var(--ambient)',
  });
  canvas.appendChild(ambient);
  backdrop.appendChild(canvas);
  page.appendChild(backdrop);
  return backdrop;
}
