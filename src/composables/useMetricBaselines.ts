import { ref, watch } from 'vue';
import { backend, isDesktop } from '../lib/bridge';
import type { MetricBaseline } from '../lib/bridge/types';
import { useSyncController } from './useSyncController';

const cache = ref<Record<string, MetricBaseline | null>>({});
const pending = new Set<string>();
let timer = 0;
let watching = false;

const flush = async () => {
  timer = 0;
  const metrics = [...pending];
  pending.clear();
  if (!metrics.length || !isDesktop()) return;
  try {
    const list = await backend.getMetricBaselines(metrics);
    const next = { ...cache.value };
    for (const metric of metrics) next[metric] = list.find((item) => item.metric === metric) ?? null;
    cache.value = next;
  } catch {
  }
};

const request = (metric: string | null | undefined) => {
  if (!metric || metric in cache.value || pending.has(metric)) return;
  pending.add(metric);
  if (!timer) timer = window.setTimeout(() => void flush(), 30);
};

export const invalidateMetricBaselines = () => {
  const known = Object.keys(cache.value);
  cache.value = {};
  for (const metric of known) request(metric);
};

export const useMetricBaselines = () => {
  if (!watching) {
    watching = true;
    const { dataRevision } = useSyncController();
    watch(dataRevision, invalidateMetricBaselines);
  }
  const baselineOf = (metric: string | null | undefined): MetricBaseline | null => {
    request(metric);
    return metric ? cache.value[metric] ?? null : null;
  };
  return { baselineOf };
};
