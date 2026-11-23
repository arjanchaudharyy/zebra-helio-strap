import { ref } from 'vue';
import { backend, isDesktop } from '../lib/bridge';
import { invalidateMetricBaselines } from './useMetricBaselines';

const excluded = ref<string[]>([]);
let loaded = false;

const load = async () => {
  if (loaded || !isDesktop()) return;
  loaded = true;
  try {
    excluded.value = await backend.metricExclusions();
  } catch {
    loaded = false;
  }
};

export const useMetricExclusions = () => {
  void load();
  const isExcluded = (metric: string | null | undefined) => Boolean(metric && excluded.value.includes(metric));
  const setExcluded = async (metric: string, value: boolean) => {
    excluded.value = await backend.setMetricExcluded(metric, value);
    invalidateMetricBaselines();
  };
  return { excluded, isExcluded, setExcluded };
};
