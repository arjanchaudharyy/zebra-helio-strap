import { ref, watch, type Ref } from 'vue';
import { SERIES_RANGE_DAYS, type SeriesRangeDays } from '../lib/metricSeries';

const STORAGE_KEY = 'zeppbridge.trend.range';
const DEFAULT_RANGE: SeriesRangeDays = 30;

const read = (): SeriesRangeDays => {
  try {
    const saved = Number(window.localStorage.getItem(STORAGE_KEY));
    return (SERIES_RANGE_DAYS as readonly number[]).includes(saved) ? (saved as SeriesRangeDays) : DEFAULT_RANGE;
  } catch {
    return DEFAULT_RANGE;
  }
};

let shared: Ref<SeriesRangeDays> | null = null;

export const useTrendRange = (): Ref<SeriesRangeDays> => {
  if (shared) return shared;
  shared = ref<SeriesRangeDays>(read());
  watch(shared, (value) => {
    try {
      window.localStorage.setItem(STORAGE_KEY, String(value));
    } catch {
    }
  });
  return shared;
};
