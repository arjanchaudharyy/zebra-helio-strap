import { defineMessages, messagesOf } from '../i18n';

export type SleepStageTone = 'deep' | 'light' | 'rem' | 'awake' | 'unknown';

export interface TimedSleepSlice {
  tone: SleepStageTone;
  start: number;
  end: number;
}

/**
 * Fill holes in a sleep timeline with an explicit unknown band.
 * The interval renderer uses this explicit band instead of extending the previous stage.
 */
export function insertSleepStageGaps(
  slices: readonly TimedSleepSlice[],
  rangeFrom: number,
  rangeTo: number,
  minGapMs = 1_000,
): TimedSleepSlice[] {
  if (!(rangeTo > rangeFrom)) return [];
  const ordered = slices
    .filter((slice) => Number.isFinite(slice.start) && Number.isFinite(slice.end) && slice.end > slice.start)
    .slice()
    .sort((a, b) => a.start - b.start || a.end - b.end);
  const result: TimedSleepSlice[] = [];
  let cursor = rangeFrom;
  for (const slice of ordered) {
    // Earlier slices own their covered time. Clip later overlaps so a minute
    // cannot be painted twice or make the time axis run backwards.
    const start = Math.max(slice.start, rangeFrom, cursor);
    const end = Math.min(slice.end, rangeTo);
    if (end <= start) continue;
    if (start - cursor >= minGapMs) {
      result.push({ tone: 'unknown', start: cursor, end: start });
    }
    const previous = result[result.length - 1];
    if (previous?.tone === slice.tone && start - previous.end < minGapMs) previous.end = end;
    else result.push({ tone: slice.tone, start, end });
    cursor = end;
  }
  if (rangeTo - cursor >= minGapMs) {
    result.push({ tone: 'unknown', start: cursor, end: rangeTo });
  }
  return result;
}

export const SLEEP_STAGE_TONES: readonly SleepStageTone[] = ['deep', 'light', 'rem', 'awake'];

export const SLEEP_STAGE_TONES_WITH_UNKNOWN: readonly SleepStageTone[] = [
  ...SLEEP_STAGE_TONES,
  'unknown',
];

const messages = defineMessages(
  {
    deep: 'Deep',
    light: 'Light',
    rem: 'REM',
    awake: 'Awake',
    unknown: 'Unknown',
  },
);

export const sleepStageLabel = (tone: SleepStageTone): string => messagesOf(messages)[tone];

export const sleepStageLabels = (): string[] => SLEEP_STAGE_TONES.map(sleepStageLabel);

export const sleepStageLabelsWithUnknown = (): string[] =>
  SLEEP_STAGE_TONES_WITH_UNKNOWN.map(sleepStageLabel);
