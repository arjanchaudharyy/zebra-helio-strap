import { CLOSE_EASE, paced } from '../timing';
import { settleCssAnimations } from '../settle';
import { catchCard, lidClose, lidOpen } from './box';
import { restOf, toLocal } from './pose';
import { reducedMotion, settled, SPRINGS, springCurve } from './spring';

const FLY_MS = paced(560);
const PEEL_MS = paced(110);
const BACK_MS = paced(480);
const PULSE_MS = 620;

export const bump = (target: Element | null | undefined): void => {
  if (!target || reducedMotion()) return;
  const { easing, duration } = springCurve(SPRINGS.pop);
  target.animate([{ scale: '1.14' }, { scale: '1' }], { duration, easing });
};

export const landPulse = (target: Element | null | undefined, tint = 'var(--accent)'): void => {
  if (!target || reducedMotion()) return;
  const rect = target.getBoundingClientRect();
  if (!rect.width && !rect.height) return;
  const radius = target instanceof HTMLElement ? getComputedStyle(target).borderRadius : '6px';
  const ring = document.createElement('div');
  ring.setAttribute('aria-hidden', 'true');
  Object.assign(ring.style, {
    position: 'fixed', left: `${rect.left - 3}px`, top: `${rect.top - 3}px`, width: `${rect.width + 6}px`, height: `${rect.height + 6}px`,
    borderRadius: radius && radius !== '0px' ? radius : '8px', pointerEvents: 'none', zIndex: '3001', opacity: '0',
    boxShadow: `0 0 0 2px ${tint}, 0 0 22px 4px color-mix(in srgb, ${tint} 45%, transparent)`,
  });
  document.body.appendChild(ring);
  void settled([ring.animate([{ opacity: 0 }, { opacity: 1, offset: 0.3 }, { opacity: 0 }], { duration: PULSE_MS, easing: 'ease-out' })]).then(() => ring.remove());
};

export const receive = (target: Element | null | undefined) => {
  target?.classList.add('is-receiving');
  return {
    land: (tint?: string) => {
      target?.classList.remove('is-receiving');
      bump(target);
      landPulse(target, tint);
    },
    cancel: () => target?.classList.remove('is-receiving'),
  };
};

const draggedOffset = (card: HTMLElement, angle: number) => {
  const m = /(-?[\d.]+)px(?:\s+(-?[\d.]+)px)?/.exec(card.style.translate || '');
  return m ? toLocal(Number(m[1]), Number(m[2] ?? 0), -angle) : { x: 0, y: 0 };
};

const ghostOf = (card: HTMLElement) => {
  const slot = card.parentElement?.classList.contains('pslot') ? card.parentElement : card;
  const resting = restOf(card);
  const shift = draggedOffset(card, resting.angle);
  const rest = { ...resting, center: { x: resting.center.x + shift.x, y: resting.center.y + shift.y } };
  const w = slot.offsetWidth || card.offsetWidth;
  const h = slot.offsetHeight || card.offsetHeight;
  const ghost = slot.cloneNode(true) as HTMLElement;
  ghost.removeAttribute('id');
  ghost.setAttribute('aria-hidden', 'true');
  ghost.inert = true;
  ghost.classList.remove('shift-l', 'shift-r', 'is-holding');
  const ghostCard = (ghost === slot ? ghost : ghost.querySelector<HTMLElement>('.pcard')) ?? ghost;
  ghostCard.classList.remove('holding', 'lifting', 'dragging', 'drop-target');
  ghost.style.setProperty('--card-w', getComputedStyle(card).getPropertyValue('--card-w') || `${card.offsetWidth}px`);
  ghostCard.style.transform = getComputedStyle(card).transform === 'none' ? '' : getComputedStyle(card).transform;
  ghostCard.style.visibility = '';
  ghostCard.style.translate = 'none';
  ghostCard.style.rotate = card.style.rotate;
  Object.assign(ghost.style, {
    position: 'fixed', left: `${(rest.center.x - w / 2).toFixed(1)}px`, top: `${(rest.center.y - h / 2).toFixed(1)}px`,
    width: `${w}px`, height: `${h}px`, margin: '0', zIndex: '3000', pointerEvents: 'none', transformOrigin: '50% 50%',
    transform: `rotate(${rest.angle.toFixed(2)}deg)`, translate: 'none', scale: 'none',
  });
  document.body.appendChild(ghost);
  settleCssAnimations(ghost);
  return { ghost, rest, w };
};

const arcTo = (rest: ReturnType<typeof restOf>, goal: DOMRect, w: number) => {
  const dx = goal.left + goal.width / 2 - rest.center.x;
  const dy = goal.top + goal.height / 2 - rest.center.y;
  const scale = Math.max(0.12, Math.min(0.45, goal.height / Math.max(w * 1.4, 1)));
  const arc = Math.min(140, 40 + Math.hypot(dx, dy) * 0.18);
  return {
    home: `rotate(${rest.angle.toFixed(2)}deg)`,
    mid: `translate(${(dx * 0.5).toFixed(1)}px, ${(dy * 0.5 - arc).toFixed(1)}px) rotate(-7deg) scale(${((1 + scale) / 2).toFixed(3)})`,
    end: `translate(${dx.toFixed(1)}px, ${dy.toFixed(1)}px) rotate(0deg) scale(${scale.toFixed(3)})`,
  };
};

export interface FlyOptions {
  keep?: boolean;
}

const PEEL_AT = 0.16;
const peelPose = (rest: ReturnType<typeof restOf>, w: number) =>
  `rotate(${(rest.angle * 0.6).toFixed(2)}deg) translateY(${(-w * 0.1).toFixed(1)}px) scale(1.06)`;

export const flyCardHome = async (card: HTMLElement, target: HTMLElement | null, onLand: () => void, options: FlyOptions = {}): Promise<boolean> => {
  if (!target || reducedMotion()) {
    onLand();
    bump(target);
    return true;
  }
  const { ghost, rest, w } = ghostOf(card);
  if (options.keep) ghost.querySelector('.pcard')?.classList.add('peel');
  else card.style.visibility = 'hidden';
  const path = arcTo(rest, target.getBoundingClientRect(), w);
  const duration = options.keep ? FLY_MS + PEEL_MS : FLY_MS;
  const caught = catchCard(target, duration);
  const frames: Keyframe[] = options.keep
    ? [
      { transform: path.home, opacity: 1, easing: 'cubic-bezier(.2, .8, .3, 1)' },
      { transform: peelPose(rest, w), opacity: 1, offset: PEEL_AT, easing: 'cubic-bezier(.4, 0, .3, 1)' },
      { transform: path.mid, opacity: 1, offset: PEEL_AT + (1 - PEEL_AT) * 0.5 },
      { opacity: 1, offset: 0.88 },
      { transform: path.end, opacity: 0 },
    ]
    : [
      { transform: path.home, opacity: 1 },
      { transform: path.mid, opacity: 1, offset: 0.5 },
      { opacity: 1, offset: 0.86 },
      { transform: path.end, opacity: 0 },
    ];
  const flight = ghost.animate(frames, { duration, easing: options.keep ? 'linear' : 'cubic-bezier(.35, 0, .25, 1)', fill: 'forwards' });
  await settled([flight]);
  const landed = flight.playState === 'finished';
  caught(landed);
  ghost.remove();
  if (landed) {
    onLand();
    bump(target);
  } else {
    card.style.visibility = '';
  }
  return landed;
};

export const flyFromBox = async (source: HTMLElement | null, card: HTMLElement, options: FlyOptions = {}): Promise<void> => {
  if (!source || reducedMotion()) {
    card.style.visibility = '';
    return;
  }
  const { ghost, rest, w } = ghostOf(card);
  if (options.keep) ghost.querySelector('.pcard')?.classList.add('peel');
  else card.style.visibility = 'hidden';
  const path = arcTo(rest, source.getBoundingClientRect(), w);
  bump(source);
  lidOpen(source);
  lidClose(source, BACK_MS * 0.3);
  const flight = ghost.animate(
    [
      { transform: path.end, opacity: 0 },
      { opacity: 1, offset: 0.16 },
      { transform: path.mid, opacity: 1, offset: 0.5 },
      ...(options.keep ? [{ transform: path.home, opacity: 1, offset: 0.9 }] : []),
      { transform: path.home, opacity: options.keep ? 0 : 1 },
    ],
    { duration: BACK_MS, easing: CLOSE_EASE, fill: 'forwards' },
  );
  await settled([flight]);
  card.style.visibility = '';
  ghost.remove();
};
