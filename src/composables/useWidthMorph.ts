import { nextTick, watch, type Ref, type WatchSource } from 'vue';
import { lessMotion } from '../lib/motion/lessMotion';

export const useWidthMorph = (
  el: Ref<HTMLElement | null>,
  source: WatchSource<unknown>,
  duration = 260,
  onSettled?: () => void,
) => {
  let running: Animation | null = null;
  let from = 0;
  const settleWith = (animation: Animation) => {
    animation.finished.then(() => {
      if (running === animation) running = null;
      onSettled?.();
    }, () => undefined);
  };
  watch(source, async () => {
    const node = el.value;
    if (!node) return;
    const reduced = lessMotion();
    if (reduced) {
      running?.cancel();
      await nextTick();
      onSettled?.();
      return;
    }
    from = node.getBoundingClientRect().width;
    running?.cancel();
    await nextTick();
    const to = node.getBoundingClientRect().width;
    if (!from || Math.abs(to - from) < 1) {
      onSettled?.();
      return;
    }
    if (Math.abs(to - from) / Math.max(from, to) > 0.3) {
      running = node.animate([{ opacity: 0.4 }, { opacity: 1 }], { duration: 140, easing: 'ease-out' });
      running.onfinish = () => { running = null; };
      onSettled?.();
      return;
    }
    const scale = node.offsetWidth ? node.getBoundingClientRect().width / node.offsetWidth : 1;
    running = node.animate(
      [
        { width: `${from / scale}px`, overflow: 'hidden' },
        { width: `${to / scale}px`, overflow: 'hidden' },
      ],
      { duration, easing: 'cubic-bezier(.2, .9, .25, 1)' },
    );
    settleWith(running);
  }, { flush: 'pre' });
};
