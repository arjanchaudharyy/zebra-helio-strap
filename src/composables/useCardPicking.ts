import { computed, nextTick, reactive, ref, watch, type Ref } from 'vue';
import { useCardCollection } from './useCardCollection';
import { flipCard, flyCardHome, flyFromBox, reducedMotion } from '../lib/motion/cards';
import { cardIdOf, pickableDates, type DeckDay, type DeckGroup } from '../lib/cards/deck';
import type { DeckSource } from '../lib/cards/sources';

const HOLD_MS = 450;
const LIFT_MS = 160;
const wait = (ms: number) => new Promise<void>((resolve) => window.setTimeout(resolve, ms));

export type CardState = 'front' | 'confirm' | 'boxed';

export const useCardPicking = (options: {
  source: () => DeckSource;
  busy: Ref<boolean>;
  cardOf: (id: string) => HTMLElement | null;
  settle?: () => Promise<void>;
  onFly?: () => void;
}) => {
  const box = useCardCollection();
  const boxEl = () => document.getElementById('card-collection-box');
  const key = () => options.source().key;
  const confirmId = ref<string | null>(null);
  const liftId = ref<string | null>(null);
  const holdId = ref<string | null>(null);
  const flying = reactive(new Set<string>());
  const returning = reactive(new Set<string>());
  const mine = computed(() => {
    const source = options.source();
    const idOf = source.idOfPick ?? ((pick: { key: string; date: string }) => (pick.key === source.key ? pick.date : null));
    const ids = box.picks.value.map(idOf).filter((id): id is string => !!id);
    return [...new Set([...ids, ...(source.extraBoxed?.() ?? [])])];
  });
  const lagBoxed = reactive(new Set<string>(mine.value));

  watch(mine, (now, before) => {
    const kept = new Set(now);
    for (const date of now) lagBoxed.add(date);
    for (const date of before ?? []) {
      if (kept.has(date) || !lagBoxed.has(date)) continue;
      const el = returning.has(date) ? null : options.cardOf(date);
      if (!el || reducedMotion()) { lagBoxed.delete(date); continue; }
      void flipCard(el, async () => { lagBoxed.delete(date); await nextTick(); });
    }
  });

  const stateOf = (id: string): CardState => {
    if (returning.has(id) || confirmId.value === id) return 'confirm';
    if (flying.has(id) || lagBoxed.has(id)) return 'boxed';
    return 'front';
  };
  const raisedId = computed(() => confirmId.value ?? liftId.value);

  const cancelConfirm = async () => {
    const id = confirmId.value;
    if (!id) return;
    const el = options.cardOf(id);
    if (!el) { confirmId.value = null; return; }
    await flipCard(el, async () => { if (confirmId.value === id) confirmId.value = null; await nextTick(); });
  };

  const turnOver = async (id: string, el: HTMLElement) => {
    liftId.value = id;
    if (!reducedMotion()) await wait(LIFT_MS);
    if (liftId.value !== id) return;
    await flipCard(el, async () => { confirmId.value = id; liftId.value = null; await nextTick(); });
  };

  const sendDay = async (day: DeckDay, el: HTMLElement) => {
    const { key: k, category } = options.source();
    const id = cardIdOf(day);
    options.onFly?.();
    const landed = flyCardHome(el, boxEl(), () => {
      box.add(day.pickKey ?? k, category, [day.date]);
      flying.delete(id);
    });
    flying.add(id);
    if (confirmId.value === id) confirmId.value = null;
    await nextTick();
    el.style.visibility = '';
    if (!(await landed)) flying.delete(id);
  };

  const sendDays = async (days: DeckDay[], el: HTMLElement) => {
    const { key: k, category } = options.source();
    const todo = days.filter((day) => day.has && stateOf(cardIdOf(day)) === 'front');
    if (!todo.length) return;
    const ids = todo.map(cardIdOf);
    const others = ids.map((id) => options.cardOf(id)).filter((card): card is HTMLElement => !!card && card !== el);
    options.onFly?.();
    for (const card of others) card.style.visibility = 'hidden';
    const landed = flyCardHome(el, boxEl(), () => {
      for (const day of todo) box.add(day.pickKey ?? k, category, [day.date]);
      for (const id of ids) flying.delete(id);
    });
    for (const id of ids) flying.add(id);
    if (confirmId.value && ids.includes(confirmId.value)) confirmId.value = null;
    await nextTick();
    el.style.visibility = '';
    for (const card of others) card.style.visibility = '';
    if (!(await landed)) for (const id of ids) flying.delete(id);
  };

  const takeBack = async (day: DeckDay, el: HTMLElement) => {
    const id = cardIdOf(day);
    returning.add(id);
    box.remove(day.pickKey ?? key(), [day.date]);
    options.source().release?.(id);
    lagBoxed.delete(id);
    await nextTick();
    await flyFromBox(boxEl(), el);
    await flipCard(el, async () => { returning.delete(id); await nextTick(); });
  };

  const inMotion = (el: HTMLElement) => el.getAnimations().some((a) => a.playState === 'running' || a.pending);

  const onDayClick = async (day: DeckDay, event: MouseEvent) => {
    const el = event.currentTarget as HTMLElement;
    const id = cardIdOf(day);
    if (!day.has || options.busy.value || flying.has(id) || returning.has(id)) return;
    if (inMotion(el) && options.settle) {
      const target = (event.target as Element | null);
      await options.settle();
      if (options.busy.value) return;
      handleDay(day, el, id, target, event.detail === 0);
      return;
    }
    handleDay(day, el, id, event.target as Element | null, event.detail === 0);
  };
  const handleDay = (day: DeckDay, el: HTMLElement, id: string, target: Element | null, keyboard = false) => {
    const state = stateOf(id);
    if (state === 'boxed') { void takeBack(day, el); return; }
    if (state === 'confirm') {
      if (keyboard || target?.closest('.back-send')) void sendDay(day, el);
      else void cancelConfirm();
      return;
    }
    if (confirmId.value) void cancelConfirm();
    void turnOver(id, el);
  };

  const settleGroup = async (el: HTMLElement) => {
    if (reducedMotion()) return;
    await nextTick();
    el.querySelector('.pcard-glow')?.animate([{ opacity: 0 }, { opacity: 0.9, offset: 0.3 }, { opacity: 0 }], { duration: 520, easing: 'ease-out' });
    el.querySelector('.pcard-badge')?.animate([{ scale: '0.4', opacity: 0 }, { scale: '1.15', opacity: 1, offset: 0.55 }, { scale: '1' }], { duration: 380, easing: 'cubic-bezier(.3, 1.3, .5, 1)' });
  };

  const pickGroup = async (group: DeckGroup, el: HTMLElement) => {
    if (options.source().kind === 'workouts') { await pickWorkoutGroup(group, el); return; }
    const dates = pickableDates(group);
    if (!dates.length) return;
    const { key: k, category } = options.source();
    if (dates.every((date) => box.has(k, date))) {
      box.remove(k, dates);
      await flyFromBox(boxEl(), el, { keep: true });
      await settleGroup(el);
      return;
    }
    options.onFly?.();
    const landed = await flyCardHome(el, boxEl(), () => box.add(k, category, dates), { keep: true });
    if (landed) await settleGroup(el);
  };

  const pickWorkoutGroup = async (group: DeckGroup, el: HTMLElement) => {
    const source = options.source();
    const boxed = new Set(mine.value);
    if (!group.days.length) return;
    if (group.days.every((day) => boxed.has(cardIdOf(day)))) {
      for (const day of group.days) {
        box.remove(day.pickKey ?? source.key, [day.date]);
        source.release?.(cardIdOf(day));
      }
      await flyFromBox(boxEl(), el, { keep: true });
      await settleGroup(el);
      return;
    }
    options.onFly?.();
    const landed = await flyCardHome(el, boxEl(), () => {
      for (const day of group.days) if (!boxed.has(cardIdOf(day))) box.add(day.pickKey ?? source.key, source.category, [day.date]);
    }, { keep: true });
    if (landed) await settleGroup(el);
  };

  const pickAll = async (days: DeckDay[] | null, groups: DeckGroup[] | null) => {
    if (options.busy.value) return;
    if (days) {
      const todo = days.filter((day) => day.has && stateOf(cardIdOf(day)) !== 'boxed');
      for (const [i, day] of todo.entries()) {
        const el = options.cardOf(cardIdOf(day));
        if (!el) continue;
        if (i) await wait(70);
        void sendDay(day, el);
      }
      return;
    }
    for (const group of groups ?? []) {
      const el = options.cardOf(group.id);
      const open = options.source().kind === 'workouts'
        ? group.days.some((day) => !mine.value.includes(cardIdOf(day)))
        : pickableDates(group).some((date) => !box.has(key(), date));
      if (el && open) void pickGroup(group, el);
    }
  };

  let hold: { id: string; x: number; y: number; timer: number; done: boolean } | null = null;
  const holdStart = (group: DeckGroup | { id: string }, event: PointerEvent, action?: (el: HTMLElement) => void) => {
    if (event.button !== 0 || options.busy.value) return;
    const el = event.currentTarget as HTMLElement;
    holdId.value = group.id;
    hold = { id: group.id, x: event.clientX, y: event.clientY, done: false, timer: window.setTimeout(() => {
      if (!hold) return;
      hold.done = true;
      holdId.value = null;
      if (action) action(el);
      else void pickGroup(group as DeckGroup, el);
    }, HOLD_MS) };
  };
  const holdMove = (event: PointerEvent) => {
    if (hold && !hold.done && Math.hypot(event.clientX - hold.x, event.clientY - hold.y) > 8) holdEnd();
  };
  const holdEnd = () => {
    if (hold && !hold.done) { clearTimeout(hold.timer); hold = null; }
    holdId.value = null;
  };
  const consumeHold = (): boolean => {
    const done = !!hold?.done;
    hold = null;
    return done;
  };

  return {
    confirmId, liftId, raisedId, holdId, stateOf,
    onDayClick, cancelConfirm, pickGroup, pickAll, sendDay, sendDays,
    holdStart, holdMove, holdEnd, consumeHold,
  };
};
