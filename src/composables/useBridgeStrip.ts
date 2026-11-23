import { ref, watch } from 'vue';
import { backend, toUserMessage } from '../lib/bridge';
import { useSyncController } from './useSyncController';
import type { DayStripRow, AdherenceDay } from '../types/timeBridge';
import { addDays, dayKey } from '../lib/aiTask/bridgeScale';
import { emptyCategories } from '../lib/aiTask/emptyCategories';
import { useAiTaskDraft } from './useAiTaskDraft';
import { isLocalMaintenanceError } from '../lib/syncDeferred';

const MAINTENANCE_RETRY_MS = 30_000;

const rows = ref<DayStripRow[]>([]);
const adherence = ref<AdherenceDay[]>([]);
const error = ref<string | null>(null);
const loading = ref(false);
const end = ref(dayKey());
let sequence = 0;
let watching = false;
let retryTimer = 0;

const load = async () => {
  const mine = ++sequence;
  window.clearTimeout(retryTimer);
  loading.value = true; error.value = null;
  try {
    const [next, compared] = await Promise.all([backend.aiTaskDayStrip(90, end.value), backend.trainingPlanAdherence(addDays(end.value, -89), end.value)]);
    if (mine !== sequence) return;
    rows.value = next; adherence.value = compared;
    useAiTaskDraft().setEmptyCategories(emptyCategories(next));
  } catch (e) {
    if (mine !== sequence) return;
    error.value = toUserMessage(e, '');
    if (isLocalMaintenanceError(e)) retryTimer = window.setTimeout(() => { void load(); }, MAINTENANCE_RETRY_MS);
  }
  finally { if (mine === sequence) loading.value = false; }
};

export const useBridgeStrip = () => {
  if (!watching) {
    watching = true;
    const { dataRevision } = useSyncController();
    watch([end, dataRevision], () => { void load(); }, { immediate: true });
  }
  return { rows, adherence, loading, error, load, end };
};
