import { ref, watch } from 'vue';
import { useCardDrag } from './useCardDrag';
import type { useCardPicking } from './useCardPicking';
import { cardIdOf, pickableDates, type DeckDay, type DeckLevel } from '../lib/cards/deck';
import { reducedMotion } from '../lib/motion/cards';

export const useCardTableDrag = (options: {
  level: () => DeckLevel | null;
  cardOf: (id: string) => HTMLElement | null;
  picking: ReturnType<typeof useCardPicking>;
  blocked: () => boolean;
  has: (date: string) => boolean;
}) => {
  const { picking } = options;
  const topLevel = options.level;
  const dayOf = (id: string) => { const level = topLevel(); return level?.kind === 'days' ? level.days.find((day) => cardIdOf(day) === id) ?? null : null; };
  const groupOf = (id: string) => { const level = topLevel(); return level?.kind === 'groups' ? level.groups.find((group) => group.id === id) ?? null : null; };
  const drag = useCardDrag({
    cardOf: options.cardOf,
    isDay: (id) => !!dayOf(id),
    canDrag: (id) => {
      if (options.blocked()) return false;
      const day = dayOf(id);
      if (day) return day.has && picking.stateOf(id) === 'front';
      const group = groupOf(id);
      return !!group && pickableDates(group).some((date) => !options.has(date));
    },
    canPileOn: (id) => { const day = dayOf(id); return !!day && day.has && picking.stateOf(id) === 'front'; },
    onStart: () => picking.holdEnd(),
    dropInBox: (id, el) => {
      const day = dayOf(id);
      if (day) { void picking.sendDay(day, el); return; }
      const group = groupOf(id);
      if (group) void picking.pickGroup(group, el);
    },
  });
  watch(() => drag.piles.value.flatMap((pile) => pile.cards).filter((id) => picking.stateOf(id) === 'boxed'), (gone) => { if (gone.length) drag.unpile(gone); });
  const scatter = () => { drag.unpile(drag.piles.value.flatMap((pile) => pile.cards), false); drag.reset(); };
  const sendPile = (id: string, el: HTMLElement) => {
    const pile = drag.pileOf(id);
    const days = (pile?.cards ?? [id]).map(dayOf).filter((day): day is DeckDay => !!day);
    void picking.sendDays(days, el);
  };
  const pileTop = (id: string) => { const pile = drag.pileOf(id); return pile ? pile.cards[pile.cards.length - 1]! : null; };
  const hintId = ref<string | null>(null);
  let hintTimer = 0;
  const nudge = (top: string) => {
    const el = options.cardOf(top);
    if (el && !reducedMotion()) {
      el.animate([{ transform: 'none' }, { transform: 'translateX(-5px) rotate(-1.5deg)' }, { transform: 'translateX(4px) rotate(1deg)' }, { transform: 'translateX(-2px)' }, { transform: 'none' }],
        { duration: 360, easing: 'ease-in-out' });
    }
    hintId.value = top;
    window.clearTimeout(hintTimer);
    hintTimer = window.setTimeout(() => { hintId.value = null; }, 1800);
  };
  const mark = (id: () => string | null, apply: (el: HTMLElement, on: boolean) => void) => watch(id, (now, before) => {
    const old = before ? options.cardOf(before) : null;
    if (old) apply(old, false);
    const el = now ? options.cardOf(now) : null;
    if (el) apply(el, true);
  }, { flush: 'post' });
  mark(() => drag.targetId.value, (el, on) => el.classList.toggle('drop-target', on));
  mark(() => picking.liftId.value, (el, on) => el.classList.toggle('lifting', on));
  mark(() => picking.holdId.value, (el, on) => {
    el.classList.toggle('holding', on);
    el.closest('.table-layer')?.classList.toggle('has-hold', on);
    if (options.level()?.kind === 'groups') el.parentElement?.classList.toggle('is-holding', on);
  });

  const onDayDown = (event: PointerEvent, day: DeckDay) => {
    const id = cardIdOf(day);
    drag.down(event, id);
    const top = pileTop(id);
    if (top) picking.holdStart({ id: top }, event, () => { const el = options.cardOf(top); if (el) sendPile(top, el); });
  };
  const onDayMove = (event: PointerEvent) => { drag.move(event); picking.holdMove(event); };
  const onDayUp = (event: PointerEvent) => { drag.up(event); picking.holdEnd(); };
  const onDayCancel = (event: PointerEvent) => { drag.cancelEvent(event); picking.holdEnd(); };
  const onDayTap = (day: DeckDay, event: MouseEvent) => {
    if (drag.consumeClick() || picking.consumeHold()) return;
    const top = pileTop(cardIdOf(day));
    if (top) { nudge(top); return; }
    void picking.onDayClick(day, event);
  };

  return { drag, scatter, hintId, onDayDown, onDayMove, onDayUp, onDayCancel, onDayTap };
};
