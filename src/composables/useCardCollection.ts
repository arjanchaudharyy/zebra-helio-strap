import { computed, ref } from 'vue';
import { backend, isDesktop } from '../lib/bridge';
import type { AiTaskCategory, CardPick } from '../lib/bridge/types';
import { WORKOUT_PREFIX } from '../lib/cards/deck';

const picks = ref<CardPick[]>([]);
const handedToTask = ref(false);
const tables = ref(0);
const dropHover = ref(false);
const pointerDragging = ref(false);
const pourRequest = ref(0);
let loaded = false;
let saveTimer = 0;

const keyOf = (pick: Pick<CardPick, 'key' | 'date'>) => `${pick.key}@${pick.date}`;

const load = async () => {
  if (loaded) return;
  loaded = true;
  if (!isDesktop()) return;
  try {
    const stored = await backend.cardCollectionGet();
    const seen = new Set(picks.value.map(keyOf));
    picks.value = [...picks.value, ...stored.filter((pick) => !seen.has(keyOf(pick)))];
  } catch {
  }
};

const save = () => {
  clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    if (!isDesktop()) return;
    void backend.cardCollectionSet(picks.value).catch(() => undefined);
  }, 300);
};

const has = (key: string, date: string) => picks.value.some((pick) => pick.key === key && pick.date === date);

const add = (key: string, category: AiTaskCategory, dates: string[]): number => {
  const seen = new Set(picks.value.map(keyOf));
  const fresh = dates.filter((date) => !seen.has(keyOf({ key, date }))).map((date) => ({ key, category, date }));
  if (!fresh.length) return 0;
  picks.value = [...picks.value, ...fresh];
  save();
  return fresh.length;
};

const remove = (key: string, dates: string[]) => {
  const drop = new Set(dates.map((date) => keyOf({ key, date })));
  const next = picks.value.filter((pick) => !drop.has(keyOf(pick)));
  if (next.length === picks.value.length) return;
  picks.value = next;
  save();
};

const clear = () => {
  if (!picks.value.length) return;
  picks.value = [];
  save();
};

export { WORKOUT_PREFIX } from '../lib/cards/deck';
const isWorkout = (key: string) => key.startsWith(WORKOUT_PREFIX);

const groups = computed(() => {
  const byKey = new Map<string, { key: string; category: AiTaskCategory; dates: string[]; items: Array<{ key: string; date: string }> }>();
  for (const pick of picks.value) {
    const groupKey = isWorkout(pick.key) ? 'workout' : pick.key;
    const group = byKey.get(groupKey) ?? { key: groupKey, category: pick.category, dates: [], items: [] };
    group.dates.push(pick.date);
    group.items.push({ key: pick.key, date: pick.date });
    byKey.set(groupKey, group);
  }
  for (const group of byKey.values()) {
    group.dates.sort();
    group.items.sort((a, b) => a.date.localeCompare(b.date));
  }
  return [...byKey.values()];
});

const byCategory = (): Map<AiTaskCategory, string[]> => {
  const out = new Map<AiTaskCategory, string[]>();
  for (const pick of picks.value) {
    if (isWorkout(pick.key)) continue;
    out.set(pick.category, [...(out.get(pick.category) ?? []), pick.date]);
  }
  return out;
};

const workoutIds = (): string[] => picks.value.filter((pick) => isWorkout(pick.key)).map((pick) => pick.key.slice(WORKOUT_PREFIX.length));

export const useCardCollection = () => {
  void load();
  return {
    picks,
    groups,
    count: computed(() => picks.value.length),
    has,
    add,
    remove,
    clear,
    byCategory,
    workoutIds,
    handedToTask,
    tableOpen: computed(() => tables.value > 0),
    dropHover,
    pointerDragging,
    pourRequest,
    setTableOpen: (open: boolean) => { tables.value = Math.max(0, tables.value + (open ? 1 : -1)); },
    clearAfterHandoff: () => {
      if (!handedToTask.value) return;
      handedToTask.value = false;
      clear();
    },
  };
};
