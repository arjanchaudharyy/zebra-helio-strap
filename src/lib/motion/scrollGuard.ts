
export const wheelPixels = (deltaY: number, deltaMode: number, pageHeight: number): number => {
  if (deltaMode === 1) return deltaY * 16;
  if (deltaMode === 2) return deltaY * pageHeight;
  return deltaY;
};

export const keyPixels = (key: string, shift: boolean, pageHeight: number): number => {
  const page = Math.max(40, pageHeight * 0.875);
  switch (key) {
    case 'PageDown': return page;
    case 'PageUp': return -page;
    case ' ': return shift ? -page : page;
    case 'ArrowDown': return 40;
    case 'ArrowUp': return -40;
    case 'End': return Number.POSITIVE_INFINITY;
    case 'Home': return Number.NEGATIVE_INFINITY;
    default: return 0;
  }
};

export const addScroll = (sum: number, next: number): number => {
  if (!Number.isFinite(next)) return next;
  return Number.isFinite(sum) ? sum + next : sum;
};

const editable = (target: EventTarget | null) =>
  target instanceof HTMLElement && (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName));

const MAX_HOLD_MS = 4000;

export interface ScrollGuard {
  release: (apply: boolean) => void;
}

export const guardScroll = (root: HTMLElement, onInput: () => void): ScrollGuard => {
  let pending = 0;
  let released = false;
  const overflow = root.style.overflowY;
  root.style.overflowY = 'hidden';
  const take = (event: Event, pixels: number) => {
    event.preventDefault();
    pending = addScroll(pending, pixels);
    onInput();
  };
  const onWheel = (event: WheelEvent) => {
    if (event.ctrlKey || !(event.target instanceof Node) || !root.contains(event.target)) return;
    if (Math.abs(event.deltaY) < Math.abs(event.deltaX)) return;
    take(event, wheelPixels(event.deltaY, event.deltaMode, root.clientHeight));
  };
  const onKey = (event: KeyboardEvent) => {
    if (event.altKey || event.ctrlKey || event.metaKey || editable(event.target)) return;
    const pixels = keyPixels(event.key, event.shiftKey, root.clientHeight);
    if (pixels) take(event, pixels);
  };
  window.addEventListener('wheel', onWheel, { capture: true, passive: false });
  window.addEventListener('keydown', onKey, true);
  const timer = window.setTimeout(() => release(false), MAX_HOLD_MS);
  const release = (apply: boolean) => {
    if (released) return;
    released = true;
    window.clearTimeout(timer);
    window.removeEventListener('wheel', onWheel, true);
    window.removeEventListener('keydown', onKey, true);
    root.style.overflowY = overflow;
    if (!apply || !pending) return;
    if (pending === Number.POSITIVE_INFINITY) root.scrollTo({ top: root.scrollHeight });
    else if (pending === Number.NEGATIVE_INFINITY) root.scrollTo({ top: 0 });
    else root.scrollBy({ top: pending, behavior: 'smooth' });
  };
  return { release };
};
