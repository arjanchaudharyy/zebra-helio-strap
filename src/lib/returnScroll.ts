import type { Router } from 'vue-router';

type Memory = { top: number; key: string | null };

const main = () => document.getElementById('main-content');

let restoring: Promise<void> | null = null;
let restored: (() => void) | null = null;
const finishRestore = () => { restored?.(); restored = null; restoring = null; };
export const whenScrollRestored = (): Promise<void> => restoring ?? Promise.resolve();
const positionOf = (): number | null => {
  const value = (window.history.state as { position?: unknown } | null)?.position;
  return typeof value === 'number' ? value : null;
};

const keyOf = (target: EventTarget | null): string | null => {
  const el = target instanceof Element ? target : null;
  const keyed = el?.closest<HTMLElement>('[data-return-key]');
  if (keyed?.dataset.returnKey) return `key:${keyed.dataset.returnKey}`;
  const link = el?.closest('a[href]');
  const href = link?.getAttribute('href');
  return href ? `href:${href}` : null;
};

export const installReturnScroll = (router: Router) => {
  const memory = new Map<number, Memory>();
  let lastKey: string | null = null;
  let current: number | null = positionOf();
  let leaving: number | null = null;

  document.addEventListener('pointerdown', (event) => { lastKey = keyOf(event.target); }, true);
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Enter' || event.key === ' ') lastKey = keyOf(event.target);
  }, true);

  router.beforeEach(() => {
    leaving = current;
    if (leaving !== null) memory.set(leaving, { top: main()?.scrollTop ?? 0, key: lastKey });
    lastKey = null;
  });

  router.afterEach((to, from, failure) => {
    if (failure) return;
    finishRestore();
    const arrived = positionOf();
    current = arrived;
    const saved = arrived !== null && leaving !== null && arrived < leaving ? memory.get(arrived) : undefined;
    const root = main();
    if (!root) return;
    if (to.name === 'Rings' && from.name === 'Rings' && to.path === from.path) return;
    if (to.matched[0] && to.matched[0] === from.matched[0]) {
      if (saved) {
        root.scrollTo({ top: saved.top });
        return;
      }
      if (from.params.card) return;
    }
    root.scrollTo({ top: 0 });
    if (!saved) return;
    const started = performance.now();
    const round = new Promise<void>((resolve) => { restored = resolve; });
    restoring = round;
    const scrollKeepingLeaving = (top: number) => {
      const before = root.scrollTop;
      root.scrollTo({ top });
      const delta = root.scrollTop - before;
      if (!delta) return;
      for (const el of root.querySelectorAll<HTMLElement>('.page-host > [class*="-leave-active"]')) {
        el.style.top = `${(Number.parseFloat(el.style.top) || 0) + delta}px`;
      }
      for (const veil of root.querySelectorAll<HTMLElement>('.page-host > * > .page-veil')) {
        veil.style.top = `${(Number.parseFloat(veil.style.top) || 0) + delta}px`;
      }
    };
    const restore = () => {
      const maxTop = root.scrollHeight - root.clientHeight;
      scrollKeepingLeaving(Math.min(saved.top, Math.max(0, maxTop)));
      if (maxTop < saved.top - 2 && performance.now() - started < 1500) {
        requestAnimationFrame(restore);
        return;
      }
      if (restoring === round) finishRestore();
    };
    requestAnimationFrame(restore);
  });
};
