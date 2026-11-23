import { backend } from '../bridge';
import type { AiTaskCategory } from '../bridge/types';
import { dayKey, daysBetween } from '../aiTask/bridgeScale';
import { daysBetweenInclusive, WORKOUT_PREFIX } from './deck';
import type { DeckDay } from './deck';
import { formatDistance, formatDuration } from '../format';
import { workoutDisplayLabel, workoutDurationMinutes, workoutIcon } from '../workouts';

export interface DeckSource {
  key: string;
  category: AiTaskCategory;
  label: string;
  tint: string;
  format: ((value: number) => string) | null;
  unit?: string;
  loadDays: (start: string, end: string) => Promise<DeckDay[]>;
  kind?: 'days' | 'workouts';
  idOfPick?: (pick: { key: string; date: string }) => string | null;
  extraBoxed?: () => string[];
  release?: (id: string) => void;
  second?: { tint: string; format: (value: number) => string; unit?: string };
}

export const PAIRED_METRICS: Readonly<Record<string, string>> = { lactate_threshold_hr: 'lactate_threshold_pace' };

export const metricSource = (options: {
  metric: string; category: AiTaskCategory; label: string; tint: string; format: (value: number) => string; unit?: string;
  second?: DeckSource['second'];
}): DeckSource => ({
  key: options.metric,
  category: options.category,
  label: options.label,
  tint: options.tint,
  format: options.format,
  unit: options.unit,
  second: options.second,
  loadDays: async (start, end) => {
    const pair = options.second ? PAIRED_METRICS[options.metric] : undefined;
    const series = await backend.getMetricSeries(pair ? [options.metric, pair] : [options.metric], daysBetween(start, deckToday()) + 1);
    const pointsOf = (metric: string) => new Map((series.find((s) => s.metric === metric)?.points ?? []).filter((p) => Number.isFinite(p.value)).map((p) => [p.date, p.value]));
    const byDate = pointsOf(options.metric);
    const second = pair ? pointsOf(pair) : null;
    const days = daysBetweenInclusive(start, end, (date) => (byDate.has(date) || second?.has(date) ? { value: byDate.get(date) ?? null, has: true } : null));
    return second ? days.map((day) => ({ ...day, value2: second.get(day.date) ?? null })) : days;
  },
});

export const categorySource = (options: {
  category: AiTaskCategory; label: string; tint: string; format: ((value: number) => string) | null; unit?: string;
}): DeckSource => ({
  key: `cat:${options.category}`,
  category: options.category,
  label: options.label,
  tint: options.tint,
  format: options.format,
  unit: options.unit,
  loadDays: async (start, end) => {
    const rows = await backend.aiTaskDayStrip(daysBetween(start, end) + 1, end);
    const cells = rows.find((row) => row.category === options.category)?.cells ?? [];
    const byDate = new Map(cells.map((cell) => [cell.date, cell]));
    return daysBetweenInclusive(start, end, (date) => {
      const cell = byDate.get(date);
      return cell ? { value: cell.value, has: cell.has || cell.value !== null } : null;
    });
  },
});

const WORKOUT_PAGE = 500;

const ICONS = { 'body-activity': 'activity', 'outdoor-cycling': 'bike', 'outdoor-run': 'run' } as const;

export const workoutSource = (options: { label: string; tint: string; picked: () => string[]; release: (id: string) => void }): DeckSource => ({
  key: 'workout',
  category: 'workout',
  label: options.label,
  tint: options.tint,
  format: null,
  kind: 'workouts',
  idOfPick: (pick) => (pick.key.startsWith(WORKOUT_PREFIX) ? pick.key.slice(WORKOUT_PREFIX.length) : null),
  extraBoxed: options.picked,
  release: options.release,
  loadDays: async (start, end) => {
    const items: Awaited<ReturnType<typeof backend.getWorkoutPage>>['items'] = [];
    for (let offset = 0; ; offset += WORKOUT_PAGE) {
      const page = await backend.getWorkoutPage(WORKOUT_PAGE, offset);
      items.push(...page.items);
      const oldest = page.items[page.items.length - 1];
      if (page.items.length < WORKOUT_PAGE || offset + page.items.length >= page.total || !oldest || dayKey(new Date(oldest.start_time)) < start) break;
    }
    return items
      .map((workout) => ({ workout, date: dayKey(new Date(workout.start_time)) }))
      .filter(({ date }) => date >= start && date <= end)
      .sort((a, b) => a.workout.start_time.localeCompare(b.workout.start_time))
      .map(({ workout, date }) => {
        const minutes = workoutDurationMinutes(workout);
        const meters = workout.distance_meters ?? 0;
        const text = [minutes !== null ? formatDuration(minutes) : null, meters > 0 ? formatDistance(meters) : null].filter(Boolean).join(' · ');
        return {
          date, value: null, has: true, id: workout.workout_id, pickKey: `${WORKOUT_PREFIX}${workout.workout_id}`,
          icon: ICONS[workoutIcon(workout)], text: text || workoutDisplayLabel(workout),
        };
      });
  },
});

export const deckToday = (): string => dayKey();
