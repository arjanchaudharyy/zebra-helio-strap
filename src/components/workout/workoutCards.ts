import type { DesignIconName } from '../DesignIcon.vue';
import type { GlyphTone } from '../../lib/glyphs';
import type { Column } from '../../lib/stage/layout';

export const WORKOUT_CARDS = [
  'summary', 'insight', 'strength-sets', 'strength-map', 'route', 'climbs',
  'chart-heart', 'chart-pace', 'chart-altitude', 'chart-cadence', 'hr-zones', 'series-status',
  'decoded', 'export', 'feel', 'provenance',
] as const;
export type WorkoutCardId = (typeof WORKOUT_CARDS)[number];

const CHART_SHRINK = 50;
export const GROW_CARDS: ReadonlyMap<string, number> = new Map([
  ['route', 160], ['chart-heart', CHART_SHRINK], ['chart-pace', CHART_SHRINK], ['chart-altitude', CHART_SHRINK], ['chart-cadence', CHART_SHRINK],
]);
export const WIDE_CARDS: ReadonlySet<string> = new Set(['summary', 'insight', 'strength-sets', 'route', 'chart-heart', 'chart-pace', 'chart-altitude', 'chart-cadence']);

export const PIP_CARDS: ReadonlySet<string> = new Set(['strength-map']);

export const CLASSIC_COLUMNS: Readonly<Record<string, Column>> = {
  summary: 'full', insight: 'full', decoded: 'side', export: 'side', feel: 'side', provenance: 'float',
};

export interface StageCard {
  id: string;
  title: string;
  icon: DesignIconName;
  tone: GlyphTone;
  rigid?: boolean;
}

export const CARD_GLYPHS: Record<string, { icon: DesignIconName; tone: GlyphTone }> = {
  summary: { icon: 'outdoor-run', tone: 'activity' },
  insight: { icon: 'training-load', tone: 'accent' },
  'strength-sets': { icon: 'power', tone: 'training' },
  'strength-map': { icon: 'body-activity', tone: 'training' },
  route: { icon: 'outdoor-run', tone: 'activity' },
  climbs: { icon: 'health-watch', tone: 'altitude' },
  'hr-zones': { icon: 'heart-rate', tone: 'heart' },
  'series-status': { icon: 'structured-data', tone: 'neutral' },
  decoded: { icon: 'structured-data', tone: 'pace' },
  export: { icon: 'document', tone: 'sleep' },
  feel: { icon: 'recovery', tone: 'heart' },
  provenance: { icon: 'database', tone: 'altitude' },
};

export const inCardOrder = (ids: Iterable<string>): string[] => {
  const set = new Set(ids);
  return WORKOUT_CARDS.filter((id) => set.has(id));
};
