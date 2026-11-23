import { computed, nextTick, shallowRef, type Ref } from 'vue';
import { groupShapeKeys, muscleLoad, type MuscleRowInput } from '../lib/anatomy/zeppMuscles';
import type { StrengthExerciseGroup, StrengthSetRow, StrengthWorkoutDetail } from '../types';
import { lessMotion } from '../lib/motion/lessMotion';
import { useMessages } from '../i18n';
import { resolveStrengthExerciseLabel, resolveStrengthMuscleLabel, strengthCatalogMessages } from '../lib/strengthCatalog.i18n';
import { workoutStrengthMessages } from '../components/workout/WorkoutStrengthCard.i18n';
import { useStrengthCatalog } from './useStrengthCatalog';
import type { MuscleChip } from '../components/workout/strength/StrengthMuscleChips.vue';

type RowKeys = 'action_code' | 'muscle_group' | 'exercise_key' | 'exercise_name' | 'secondary_muscle_groups';
export type StrengthRowLike = Pick<StrengthSetRow, RowKeys> | Pick<StrengthExerciseGroup, RowKeys>;
export type FocusKind = 'row' | 'shape' | 'group';
type Focus = { kind: FocusKind; key: string };

const MUSCLE_COLORS: Record<string, string> = {
  chest: 'var(--heart)',
  back: 'var(--accent)',
  shoulders: 'var(--training)',
  biceps: 'var(--pace)',
  triceps: 'var(--activity)',
  forearms: 'var(--altitude)',
  core: 'var(--calories)',
  glutes: 'var(--sleep)',
  legs: 'color-mix(in srgb, var(--training) 65%, var(--accent))',
};
const NONE: ReadonlySet<string> = new Set();

export const useStrengthDetail = (strength: Ref<StrengthWorkoutDetail>) => {
  const t = useMessages(workoutStrengthMessages);
  const catalog = useMessages(strengthCatalogMessages);
  const zepp = useStrengthCatalog(computed(() => !!(strength.value.sets.length || strength.value.exercise_groups.length)));

  const primaryOf = (row: StrengthRowLike): string | null =>
    row.muscle_group ?? zepp.value.get(row.action_code)?.primary_groups[0] ?? null;

  const secondaryOf = (row: StrengthRowLike): string[] => {
    const primary = primaryOf(row);
    const list = row.secondary_muscle_groups?.length
      ? row.secondary_muscle_groups
      : (zepp.value.get(row.action_code)?.secondary_groups ?? []);
    return list.filter((group) => group !== primary);
  };

  const exerciseOf = (row: StrengthRowLike): { name: string; unknownCode: number | null } => {
    const label = resolveStrengthExerciseLabel(row.exercise_key, row.action_code, catalog.value, zepp.value.get(row.action_code)?.name, row.exercise_name);
    return label === catalog.value.unknownAction(row.action_code)
      ? { name: t.value.unknownExercise, unknownCode: row.action_code }
      : { name: label, unknownCode: null };
  };

  const weightedRows = computed<Array<{ id: string; row: StrengthRowLike; sets: number }>>(() =>
    strength.value.sets.length > 0
      ? strength.value.sets.map((row) => ({ id: `s${row.set_index}`, row, sets: 1 }))
      : strength.value.exercise_groups.map((row, idx) => ({ id: `g${idx}`, row, sets: row.sets })).filter(({ sets }) => sets > 0),
  );

  const load = computed(() =>
    muscleLoad(weightedRows.value.map(({ id, row, sets }): MuscleRowInput => {
      const entry = zepp.value.get(row.action_code);
      return {
        id, sets,
        mainPositions: entry?.main_positions ?? [],
        subPositions: entry?.sub_positions ?? [],
        primaryGroup: primaryOf(row),
        secondaryGroups: secondaryOf(row),
      };
    })),
  );

  const rowGroups = computed(() =>
    new Map(weightedRows.value.map(({ id, row }) => {
      const primary = primaryOf(row);
      return [id, new Set([...(primary ? [primary] : []), ...secondaryOf(row)])];
    })),
  );

  const hover = shallowRef<Focus | null>(null);
  const pinned = shallowRef<Focus | null>(null);
  const focus = computed(() => hover.value ?? pinned.value);
  const tableWrap = shallowRef<HTMLElement | null>(null);

  const hoverOn = (kind: FocusKind, key: string | null) => {
    if (key) hover.value = { kind, key };
    else if (hover.value?.kind === kind) hover.value = null;
  };

  const revealFirstLitRow = () => {
    const row = tableWrap.value?.querySelector<HTMLElement>('tbody tr.lit');
    if (!row) return;
    const rect = row.getBoundingClientRect();
    const scroller = row.closest<HTMLElement>('[data-stage-scroll]');
    const view = scroller ? scroller.getBoundingClientRect() : { top: 96, bottom: window.innerHeight - 16 };
    if (rect.top >= view.top && rect.bottom <= view.bottom) return;
    row.scrollIntoView({ block: 'center', behavior: lessMotion() ? 'auto' : 'smooth' });
  };

  const pin = (kind: FocusKind, key: string) => {
    const same = pinned.value?.kind === kind && pinned.value.key === key;
    pinned.value = same ? null : { kind, key };
    if (!same && kind !== 'row') void nextTick(revealFirstLitRow);
  };

  const groupShapes = (group: string): ReadonlySet<string> => {
    const present = groupShapeKeys(group).filter((key) => load.value.byShape.has(key));
    if (present.length) return new Set(present);
    const out = new Set<string>();
    for (const [id, groups] of rowGroups.value) {
      if (!groups.has(group)) continue;
      const shapes = load.value.byRow.get(id);
      shapes?.primary.forEach((key) => out.add(key));
      shapes?.secondary.forEach((key) => out.add(key));
    }
    return out;
  };

  const mapRow = computed(() => (focus.value?.kind === 'row' ? focus.value.key : null));
  const mapShape = computed(() => (focus.value?.kind === 'shape' ? focus.value.key : null));
  const mapShapes = computed(() => (focus.value?.kind === 'group' ? groupShapes(focus.value.key) : null));

  const rowClass = (id: string) => {
    const f = focus.value;
    if (!f) return undefined;
    if (f.kind === 'row') return f.key === id ? 'lit' : undefined;
    if (f.kind === 'shape') return load.value.byShape.get(f.key)?.rows.has(id) ? 'lit' : 'dim';
    return rowGroups.value.get(id)?.has(f.key) ? 'lit' : 'dim';
  };

  const chipOf = (key: string, sets: number): MuscleChip => ({
    key, sets, label: resolveStrengthMuscleLabel(key, catalog.value) ?? key, color: MUSCLE_COLORS[key] ?? 'var(--accent)',
  });
  const tally = (pick: (row: StrengthRowLike) => string[]) => {
    const counts = new Map<string, number>();
    for (const { row, sets } of weightedRows.value) for (const group of pick(row)) counts.set(group, (counts.get(group) ?? 0) + sets);
    return Array.from(counts.entries()).sort((a, b) => b[1] - a[1]).map(([key, sets]) => chipOf(key, sets));
  };
  const muscleShares = computed<MuscleChip[]>(() => tally((row) => { const g = primaryOf(row); return g ? [g] : []; }));
  const secondaryShares = computed<MuscleChip[]>(() => tally(secondaryOf));

  const litGroups = computed<ReadonlySet<string>>(() => {
    const f = focus.value;
    if (!f) return NONE;
    if (f.kind === 'group') return new Set([f.key]);
    if (f.kind === 'row') return rowGroups.value.get(f.key) ?? NONE;
    const all = [...muscleShares.value, ...secondaryShares.value].map((chip) => chip.key);
    return new Set(all.filter((group) => groupShapeKeys(group).includes(f.key)));
  });

  const hasMap = computed(() => muscleShares.value.length > 0 && load.value.byShape.size > 0);

  const muscleTitle = (row: StrengthRowLike): string | undefined => {
    const names = secondaryOf(row).map((group) => resolveStrengthMuscleLabel(group, catalog.value)).filter((name): name is string => !!name);
    return names.length ? `${t.value.secondaryMuscles}: ${names.join(' · ')}` : undefined;
  };
  const muscleOf = (row: StrengthRowLike) => resolveStrengthMuscleLabel(primaryOf(row), catalog.value);

  return {
    exerciseOf, muscleOf, muscleTitle, load, focus, tableWrap, hoverOn, pin, mapRow, mapShape, mapShapes, rowClass,
    muscleShares, secondaryShares, litGroups, hasMap,
  };
};

export type StrengthDetail = ReturnType<typeof useStrengthDetail>;
