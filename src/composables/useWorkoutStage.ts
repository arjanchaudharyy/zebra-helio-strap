import { ref, shallowRef, watch, type Ref } from 'vue';
import * as S from '../lib/stage/state';
import { clearStage, defaultState, loadStage, loadView, saveStage, saveView, type SportClass, type StageView } from '../lib/stage/prefs';

export const useWorkoutStage = (cards: Ref<readonly string[]>, cls: Ref<SportClass>, wide?: (id: string) => boolean) => {
  const state = shallowRef<S.StageState>(loadStage(cls.value, cards.value));
  const draft = shallowRef<S.StageState | null>(null);
  const view = ref<StageView>(loadView());
  const stripOpen = ref(false);
  const front = ref<string | undefined>(undefined);

  watch([() => cards.value.join(','), cls], () => { draft.value = null; state.value = loadStage(cls.value, cards.value); });

  const commit = (next: S.StageState) => {
    if (next === state.value || JSON.stringify(next) === JSON.stringify(state.value)) return false;
    state.value = next;
    if (!saveStage(cls.value, next)) console.warn('[stage] layout not saved (storage unavailable); the default layout is used next time');
    return true;
  };

  const staged = (next: S.StageState, lead?: string) => {
    const changed = commit(next);
    if (lead && next.stage.ids.includes(lead)) front.value = lead;
    else if (!next.stage.ids.includes(front.value ?? '')) front.value = undefined;
    return changed;
  };

  return {
    state,
    draft,
    wide,
    commitDraft: () => {
      const next = draft.value;
      draft.value = null;
      return next ? staged(next) : false;
    },
    view,
    setView: (next: StageView) => { view.value = next; saveView(next); stripOpen.value = false; },
    stripOpen,
    front,
    bringGroup: (group: number) => staged(S.bringGroup(state.value, group), state.value.strip[group]?.ids[0]),
    cycle: (dir: 1 | -1) => staged(S.cycle(state.value, dir)),
    addToStage: (id: string, at?: number) => staged(S.addToStage(state.value, id, at, wide), id),
    removeFromStage: (id: string) => staged(S.removeFromStage(state.value, id, wide)),
    solo: (id: string) => staged(S.solo(state.value, id), id),
    fullscreen: (id: string) => staged(S.fullscreen(state.value, id), id),
    setSnap: (snap: number) => staged(S.setSnap(state.value, snap)),
    setRatio: (ratio: number) => staged(S.setRatio(state.value, ratio)),
    setSize: (patch: S.SizePatch) => staged(S.setSize(state.value, patch)),
    swapOnStage: (a: string, b: string) => staged(S.swapOnStage(state.value, a, b)),
    hide: (id: string) => staged(S.hide(state.value, id)),
    focusCard: (id: string) => staged(S.focusCard(state.value, id), id),
    setFront: (id: string) => { front.value = id; },
    reset: () => {
      clearStage(cls.value);
      draft.value = null;
      state.value = defaultState(cls.value, cards.value);
      front.value = undefined;
    },
  };
};

export type WorkoutStage = ReturnType<typeof useWorkoutStage>;
