import { CLOSE_EASE, OPEN_EASE, paced } from '../timing';
import { fanBack, stackCards, type Landing } from './deal';
import { angleOf, centerOfRect, restOf } from './pose';
import { animateFromNow, cancelOn } from './reversible';
import { reducedMotion, settled } from './spring';
import { settleCssAnimations } from '../settle';

const RECEDE_MS = paced(340);
const PULL_MS = paced(440);
const RECEDE_SCALE = 0.42;
const RECEDE_OPACITY = 0.4;
const CORNER_PAD = 10;
const BLUR_SWAP_MS = paced(220);
const BLUR = 'blur(5px) saturate(.9)';

const blurredCopy = (layer: HTMLElement): HTMLElement => {
  const copy = layer.cloneNode(true) as HTMLElement;
  copy.removeAttribute('id');
  copy.setAttribute('aria-hidden', 'true');
  copy.inert = true;
  copy.classList.add('blurred-copy');
  const from = [...layer.querySelectorAll<HTMLElement>('.pcard, .pslot')];
  const to = [...copy.querySelectorAll<HTMLElement>('.pcard, .pslot')];
  from.forEach((el, i) => {
    const style = getComputedStyle(el);
    const twin = to[i];
    if (!twin) return;
    twin.style.transform = style.transform;
    twin.style.opacity = style.opacity;
    twin.style.transition = 'none';
  });
  const style = getComputedStyle(layer);
  Object.assign(copy.style, { transform: style.transform, transformOrigin: style.transformOrigin, opacity: '0', filter: BLUR, pointerEvents: 'none', transition: 'none' });
  layer.before(copy);
  settleCssAnimations(copy);
  return copy;
};

export interface Receded {
  pile: Landing;
  now: () => Landing;
  restore: () => Promise<void>;
  drop: () => void;
  fadeOut: (duration: number) => Promise<void>;
  fadeIn: (duration: number) => Promise<void>;
}

export const recedeLayer = (layer: HTMLElement, pileCard: HTMLElement, live: () => boolean = () => true): Receded & { ready: Promise<void> } => {
  const slot = pileCard.parentElement as HTMLElement | null;
  const rest = restOf(pileCard);
  const pile: Landing = { rect: new DOMRect(rest.center.x - rest.width / 2, rest.center.y - (rest.width * 1.4) / 2, rest.width, rest.width * 1.4), kind: 'pile', angle: rest.angle, width: rest.width };
  const others = [...layer.querySelectorAll<HTMLElement>('.pcard')].filter((card) => card !== pileCard);
  const box = layer.getBoundingClientRect();
  const origin = `${(rest.center.x - box.left).toFixed(1)}px ${(rest.center.y - box.top).toFixed(1)}px`;
  if (slot) slot.style.zIndex = '5';
  layer.inert = true;
  layer.style.transformOrigin = origin;
  const reduced = reducedMotion();
  let restoring = false;
  let copy: HTMLElement | null = null;

  const drop = () => {
    copy?.remove();
    copy = null;
    cancelOn(layer);
    for (const card of others) cancelOn(card);
    layer.inert = false;
    layer.style.transformOrigin = '';
    if (slot) slot.style.zIndex = '';
  };
  const fadeOut = async (duration: number) => {
    restoring = true;
    const els = [layer, copy].filter((el): el is HTMLElement => !!el);
    const fades = els.map((el) => animateFromNow(el, { opacity: 0 }, { duration, easing: CLOSE_EASE, fill: 'forwards' }));
    await settled(fades);
  };
  const fadeIn = async (duration: number) => {
    const el = copy ?? layer;
    await settled([animateFromNow(el, { opacity: RECEDE_OPACITY }, { duration, easing: OPEN_EASE, fill: 'forwards' })]);
  };
  const restore = async () => {
    restoring = true;
    if (copy) {
      const swap = animateFromNow(copy, { opacity: 0 }, { duration: BLUR_SWAP_MS, easing: 'ease-out', fill: 'forwards' }, ['opacity']);
      const sharp = animateFromNow(layer, { opacity: RECEDE_OPACITY }, { duration: BLUR_SWAP_MS, easing: 'ease-out', fill: 'forwards' });
      await settled([swap, sharp]);
      copy?.remove();
      copy = null;
    }
    const forward = animateFromNow(layer, { transform: 'none', opacity: 1 }, { duration: reduced ? 160 : PULL_MS, easing: CLOSE_EASE, fill: 'forwards' });
    await settled([forward]);
    if (!reduced) await fanBack(others);
    drop();
  };
  const now = (): Landing => {
    const rect = pileCard.getBoundingClientRect();
    const scale = layer.getBoundingClientRect().width / Math.max(1, layer.offsetWidth);
    const width = rest.width * scale;
    const c = centerOfRect(rect);
    return { rect: new DOMRect(c.x - width / 2, c.y - (width * 1.4) / 2, width, width * 1.4), kind: 'pile', angle: angleOf(slot), width };
  };
  const ready = (async () => {
    if (reduced) {
      await settled([layer.animate([{ opacity: 1 }, { opacity: RECEDE_OPACITY }], { duration: 160, fill: 'forwards' })]);
      return;
    }
    await stackCards(others, rest.center, rest.angle);
    if (!live() || restoring) return;
    const table = (layer.parentElement ?? layer).getBoundingClientRect();
    const w = rest.width * RECEDE_SCALE;
    const corner = { x: table.left + CORNER_PAD + w / 2, y: table.top + CORNER_PAD + (w * 1.4) / 2 };
    const tx = corner.x - rest.center.x;
    const ty = corner.y - rest.center.y;
    const back = animateFromNow(layer, { transform: `translate(${tx.toFixed(1)}px, ${ty.toFixed(1)}px) scale(${RECEDE_SCALE})`, opacity: RECEDE_OPACITY }, { duration: RECEDE_MS + 80, easing: OPEN_EASE, fill: 'forwards' });
    await settled([back]);
    if (!live() || restoring || back.playState !== 'finished') return;
    copy = blurredCopy(layer);
    const blurIn = copy.animate([{ opacity: 0 }, { opacity: RECEDE_OPACITY }], { duration: BLUR_SWAP_MS, easing: 'ease-out', fill: 'forwards' });
    animateFromNow(layer, { opacity: 0 }, { duration: BLUR_SWAP_MS, easing: 'ease-out', fill: 'forwards' });
    await settled([blurIn]);
  })();
  return { pile, now, restore, drop, fadeOut, fadeIn, ready };
};
