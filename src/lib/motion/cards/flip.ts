import { reducedMotion, SPRINGS, springCurve } from './spring';
import { paced } from '../timing';

const HALF_MS = paced(120);

export const flipCard = async (card: HTMLElement, swap: () => void | Promise<void>): Promise<void> => {
  if (reducedMotion()) {
    await swap();
    return;
  }
  const close = card.animate([{ transform: 'none' }, { transform: 'scaleX(.04) scaleY(1.03)' }], { duration: HALF_MS, easing: 'cubic-bezier(.55, 0, .9, .45)', fill: 'forwards' });
  await close.finished.catch(() => undefined);
  await swap();
  const { easing, duration } = springCurve(SPRINGS.flip);
  const open = card.animate([{ transform: 'scaleX(.04) scaleY(1.03)' }, { transform: 'none' }], { duration, easing });
  close.cancel();
  await open.finished.catch(() => undefined);
};
