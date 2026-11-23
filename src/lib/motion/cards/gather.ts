import { CLOSE_EASE, OPEN_EASE, paced } from '../timing';
import { lessMotion } from '../lessMotion';
import { settleCssAnimations } from '../settle';

const GATHER_MS = paced(460);
const STAGGER_MS = paced(45);
const FLY_MS = paced(520);
const RETURN_MS = paced(440);

export interface CardFlight {
  land: (target: HTMLElement | null) => Promise<void>;
  returnHome: () => Promise<void>;
}

const reduced = () => lessMotion();
const finished = (animations: Animation[]) => Promise.all(animations.map((a) => a.finished.catch(() => undefined))).then(() => undefined);

export const gatherCards = (cards: HTMLElement[]): CardFlight => {
  if (reduced() || !cards.length) {
    return { land: async () => undefined, returnHome: async () => undefined };
  }
  const rects = cards.map((card) => card.getBoundingClientRect());
  const left = Math.min(...rects.map((r) => r.left));
  const right = Math.max(...rects.map((r) => r.right));
  const top = Math.min(...rects.map((r) => r.top));
  const bottom = Math.max(...rects.map((r) => r.bottom));
  const center = { x: (left + right) / 2, y: (top + bottom) / 2 };
  const hidden = cards.map((card) => card.animate([{ opacity: 0 }, { opacity: 0 }], { duration: 1, fill: 'forwards' }));
  const clones = cards.map((card, i) => {
    const rect = rects[i]!;
    const clone = card.cloneNode(true) as HTMLElement;
    clone.removeAttribute('href');
    clone.setAttribute('aria-hidden', 'true');
    Object.assign(clone.style, {
      position: 'fixed', left: `${rect.left}px`, top: `${rect.top}px`, width: `${rect.width}px`, height: `${rect.height}px`,
      margin: '0', zIndex: String(2000 + i), pointerEvents: 'none', translate: '', animation: 'none',
      background: 'var(--mat-raised)', boxShadow: 'var(--mat-raised-rim), 0 12px 26px -16px rgba(0,0,0,.6)', borderRadius: '12px',
    });
    document.body.appendChild(clone);
    settleCssAnimations(clone);
    return clone;
  });
  const stacked = (i: number) => {
    const rect = rects[i]!;
    const dx = center.x - (rect.left + rect.width / 2) + (i - clones.length / 2) * 1.5;
    const dy = center.y - (rect.top + rect.height / 2) - i * 1.5;
    const tilt = ((i * 37) % 9) - 4;
    return `translate(${dx.toFixed(1)}px, ${dy.toFixed(1)}px) rotate(${tilt}deg) scale(.9)`;
  };
  const gathered = clones.map((clone, i) => clone.animate(
    [{ transform: 'none' }, { transform: stacked(i) }],
    { duration: GATHER_MS, delay: i * STAGGER_MS, easing: OPEN_EASE, fill: 'forwards' },
  ));
  const cleanup = () => {
    for (const clone of clones) clone.remove();
    for (const animation of hidden) animation.cancel();
  };

  return {
    land: async (target) => {
      await finished(gathered);
      const goal = target?.getBoundingClientRect();
      const flights = clones.map((clone, i) => {
        const rect = rects[i]!;
        const to = goal
          ? `translate(${(goal.left + goal.width / 2 - (rect.left + rect.width / 2)).toFixed(1)}px, ${(goal.top + goal.height / 2 - (rect.top + rect.height / 2)).toFixed(1)}px) rotate(0deg) scale(.12)`
          : `${stacked(i)} translateY(-40px)`;
        return clone.animate(
          [{ transform: stacked(i), opacity: 1 }, { transform: to, opacity: 0.9, offset: 0.8 }, { transform: to, opacity: 0 }],
          { duration: FLY_MS, delay: (clones.length - 1 - i) * 18, easing: CLOSE_EASE, fill: 'forwards' },
        );
      });
      await finished(flights);
      for (const card of cards) card.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 260, easing: 'ease-out' });
      cleanup();
    },
    returnHome: async () => {
      await finished(gathered);
      const back = clones.map((clone, i) => clone.animate(
        [{ transform: stacked(i) }, { transform: 'none' }],
        { duration: RETURN_MS, delay: i * STAGGER_MS, easing: OPEN_EASE, fill: 'forwards' },
      ));
      await finished(back);
      cleanup();
    },
  };
};
