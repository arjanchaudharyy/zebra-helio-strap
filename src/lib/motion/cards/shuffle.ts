import { reducedMotion, settled } from './spring';
import { paced } from '../timing';

const SHUFFLE_MS = paced(420);
const CUT_MS = paced(480);

export const shuffleCards = async (cards: HTMLElement[]): Promise<void> => {
  if (!cards.length || reducedMotion()) return;
  await settled(cards.map((card, i) => {
    const side = i % 2 === 0 ? -1 : 1;
    const dx = side * (22 + ((i * 13) % 18));
    const tilt = side * (3 + ((i * 7) % 5));
    return card.animate(
      [{ translate: '0 0', rotate: '0deg' }, { translate: `${dx}px 0`, rotate: `${tilt}deg`, offset: 0.45 }, { translate: '0 0', rotate: '0deg' }],
      { duration: SHUFFLE_MS, delay: i * 22, easing: 'cubic-bezier(.45, 0, .2, 1)' },
    );
  }));
};

export const cutDeck = async (cards: HTMLElement[]): Promise<void> => {
  if (cards.length < 2 || reducedMotion()) return;
  const half = Math.ceil(cards.length / 2);
  await settled(cards.map((card, i) => {
    const top = i < half;
    const lifted = top ? { translate: '34px -18px', rotate: '4deg' } : { translate: '-14px 6px', rotate: '-2deg' };
    return card.animate(
      [{ translate: '0 0', rotate: '0deg' }, { ...lifted, offset: 0.5 }, { translate: '0 0', rotate: '0deg' }],
      { duration: CUT_MS, easing: 'cubic-bezier(.4, .6, .2, 1)' },
    );
  }));
};
