/** Detail routes keep the tab they were opened from. */
export const navigationBranch = (path: string): string => {
  if (path === '/ai' || path.startsWith('/ai/')) return '/ai';
  if (path === '/settings' || path.startsWith('/settings/') || path === '/health-check' || path.startsWith('/devices')) return '/settings';
  return '/';
};

export const historyBackPath = (): string | null => {
  const back = (window.history.state as { back?: unknown } | null)?.back;
  return typeof back === 'string' && back ? back : null;
};

export interface BackDestination {
  path: string;
  viaHistory: boolean;
}

export const backDestination = (current: string, back: string | null): BackDestination => (
  back && back !== current ? { path: back, viaHistory: true } : { path: navigationBranch(current), viaHistory: false }
);

export const cardCloseDestination = (back: string | null): BackDestination => {
  if (!back || /^\/settings\/[^/?#]+/.test(back)) return { path: '/settings', viaHistory: false };
  return { path: back, viaHistory: true };
};

export const TAB_ORDER = ['/', '/ai', '/settings'] as const;

export type PageMotion = 'forward' | 'back' | 'left' | 'right' | 'expand' | 'collapse' | 'dismiss' | 'none';

const isTabRoot = (path: string) => (TAB_ORDER as readonly string[]).includes(path);

export const pageMotion = (from: string, to: string): PageMotion => {
  if (from === to) return 'none';
  const a = navigationBranch(from);
  const b = navigationBranch(to);
  if (a === b) {
    const da = isTabRoot(from) ? 0 : 1;
    const db = isTabRoot(to) ? 0 : 1;
    if (db > da) return 'forward';
    if (db < da) return 'back';
    return 'forward';
  }
  if (!isTabRoot(from)) return 'dismiss';
  return TAB_ORDER.indexOf(b as (typeof TAB_ORDER)[number]) > TAB_ORDER.indexOf(a as (typeof TAB_ORDER)[number])
    ? 'left'
    : 'right';
};

export interface SegmentStop<T = string> {
  left: number;
  width: number;
  value: T;
  top?: number;
  height?: number;
}

export function dragThumb<T>(stops: SegmentStop<T>[], center: number, velocity = 0) {
  if (!stops.length) return { left: 0, width: 0 };
  const centers = stops.map((stop) => stop.left + stop.width / 2);
  const clamped = Math.max(centers[0], Math.min(centers[centers.length - 1], center));
  const right = Math.max(1, centers.findIndex((value) => value >= clamped));
  const a = stops[right - 1];
  const b = stops[right] ?? a;
  const span = (centers[right] ?? centers[right - 1]) - centers[right - 1] || 1;
  const fraction = Math.max(0, Math.min(1, (clamped - centers[right - 1]) / span));
  const width = a.width + (b.width - a.width) * fraction + Math.min(10, Math.abs(velocity) * 6);
  const maxLeft = stops[stops.length - 1].left + stops[stops.length - 1].width - width;
  return { left: Math.max(stops[0].left, Math.min(maxLeft, clamped - width / 2)), width };
}

export const FLICK_VELOCITY = 0.6;

export function snapStop<T>(stops: SegmentStop<T>[], center: number, velocity: number): SegmentStop<T> {
  const nearestTo = (x: number) => stops.reduce((best, stop) =>
    Math.abs(stop.left + stop.width / 2 - x) < Math.abs(best.left + best.width / 2 - x) ? stop : best);
  const here = nearestTo(center);
  if (Math.abs(velocity) < FLICK_VELOCITY) return here;
  const from = stops.indexOf(here);
  const to = stops.indexOf(nearestTo(center + Math.max(-90, Math.min(90, velocity * 110))));
  return stops[from + Math.max(-1, Math.min(1, to - from))] ?? here;
}

