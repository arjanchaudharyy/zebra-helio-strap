import { computed, type ComputedRef } from 'vue';
import { useCardCollection } from './useCardCollection';
import { useCardsText } from '../components/cards/cards.i18n';
import { cardIdOf, groupSummary, monthsOf, quarterOf, type DeckDay, type DeckGroup, type DeckLevel, type GroupCoverage } from '../lib/cards/deck';
import { groupCoverage, type CoverageContext } from '../lib/cards/coverage';
import type { DeckSource } from '../lib/cards/sources';

export interface GroupView {
  title: string;
  top: string;
  value: string | null;
  dots: Array<boolean | 'picked'>;
  empty: boolean;
  coverage: GroupCoverage;
  badge: string | null;
  picked: 'some' | 'all' | null;
  aria: string;
  description: string;
}

interface GroupFacts { summary: ReturnType<typeof groupSummary>; pickable: DeckDay[]; buckets: DeckDay[][] }
const facts = new WeakMap<DeckGroup, GroupFacts>();
const factsOf = (group: DeckGroup): GroupFacts => {
  let hit = facts.get(group);
  if (!hit) {
    const buckets = group.kind === 'week' || group.kind === 'month' ? group.days.map((day) => [day]) : monthsOf(group.days).map((month) => month.days);
    hit = { summary: groupSummary(group), pickable: group.days.filter((day) => day.has), buckets };
    facts.set(group, hit);
  }
  return hit;
};

const sameView = (a: GroupView, b: GroupView) => a.title === b.title && a.top === b.top && a.value === b.value && a.empty === b.empty && a.coverage === b.coverage
  && a.badge === b.badge && a.picked === b.picked && a.aria === b.aria && a.description === b.description
  && a.dots.length === b.dots.length && a.dots.every((dot, i) => dot === b.dots[i]);

export const useGroupViews = (options: {
  levels: () => DeckLevel[];
  source: () => DeckSource;
  coverage: () => CoverageContext | null;
  numberText: (value: number | null) => string | null;
  md: (date: string) => string;
  monthName: (date: string) => string;
  yearMonth: (date: string) => string;
}) => {
  const t = useCardsText();
  const box = useCardCollection();

  const groupTitle = (group: DeckGroup) => {
    if (group.kind === 'year') return group.start.slice(0, 4);
    if (group.kind === 'quarter') return t.value.quarter(quarterOf(group.start));
    if (group.kind === 'month') return options.monthName(group.start);
    return group.start === group.end ? options.md(group.start) : t.value.weekRange(options.md(group.start), options.md(group.end));
  };
  const groupTop = (group: DeckGroup) => {
    if (group.kind === 'year') return t.value.yearTop;
    return group.kind === 'month' || group.kind === 'quarter' ? group.start.slice(0, 4) : options.yearMonth(group.start);
  };

  let previous = new Map<string, GroupView>();
  const views: ComputedRef<Map<string, GroupView>> = computed(() => {
    const source = options.source();
    const workouts = source.kind === 'workouts';
    const picked = workouts
      ? new Set([...box.picks.value.map((pick) => source.idOfPick?.(pick) ?? null).filter((id): id is string => !!id), ...(source.extraBoxed?.() ?? [])])
      : new Set(box.picks.value.filter((pick) => pick.key === source.key).map((pick) => pick.date));
    const isPicked = (day: DeckDay) => picked.has(workouts ? cardIdOf(day) : day.date);
    const ctx = options.coverage();
    const next = new Map<string, GroupView>();
    for (const level of options.levels()) {
      if (level.kind !== 'groups') continue;
      for (const group of level.groups) {
        const { summary, pickable, buckets } = factsOf(group);
        const title = groupTitle(group);
        const average = options.numberText(summary.average);
        const sub = workouts ? t.value.workoutCount(group.days.length) : t.value.recordedOf(summary.recorded, summary.total);
        const n = pickable.reduce((count, day) => count + (isPicked(day) ? 1 : 0), 0);
        const view: GroupView = {
          title,
          top: groupTop(group),
          value: workouts ? t.value.workoutCount(group.days.length) : average === null ? null : t.value.average(average),
          dots: buckets.map((days) => (days.some((day) => day.has && isPicked(day)) ? 'picked' : days.some((day) => day.has))),
          empty: !summary.recorded,
          coverage: groupCoverage(group, ctx),
          badge: n ? t.value.pickedOf(n, pickable.length) : null,
          picked: !n ? null : n >= pickable.length ? 'all' : 'some',
          aria: t.value.groupAria(title, sub),
          description: n && n === pickable.length ? t.value.holdToUnpick : t.value.holdToPick,
        };
        const before = previous.get(group.id);
        next.set(group.id, before && sameView(before, view) ? before : view);
      }
    }
    previous = next;
    return next;
  });

  return { views, groupTitle };
};
