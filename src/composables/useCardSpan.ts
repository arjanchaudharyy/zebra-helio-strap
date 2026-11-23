import { ref } from 'vue';
import { cardIdOf, spanBetween, type DeckLevel } from '../lib/cards/deck';
import type { useCardPicking } from './useCardPicking';

export const useCardSpan = (options: {
  level: () => DeckLevel | null;
  picking: ReturnType<typeof useCardPicking>;
  blocked: () => boolean;
}) => {
  const on = ref(false);
  const from = ref<string | null>(null);
  const reset = () => { on.value = false; from.value = null; };
  const toggle = () => { on.value = !on.value; from.value = null; };

  const tap = (event: MouseEvent, id: string): boolean => {
    const level = options.level();
    if (!level || options.blocked()) return on.value;
    const ids = level.kind === 'days' ? level.days.map(cardIdOf) : level.groups.map((group) => group.id);
    const anchor = from.value && ids.includes(from.value) ? from.value : null;
    if ((!on.value && !event.shiftKey) || !anchor || (on.value && anchor === id)) {
      from.value = id;
      return on.value;
    }
    if (level.kind === 'days') void options.picking.pickAll(spanBetween(level.days, cardIdOf, anchor, id), null);
    else void options.picking.pickAll(null, spanBetween(level.groups, (group) => group.id, anchor, id));
    from.value = on.value ? null : id;
    on.value = false;
    return true;
  };

  return { on, from, reset, toggle, tap };
};
