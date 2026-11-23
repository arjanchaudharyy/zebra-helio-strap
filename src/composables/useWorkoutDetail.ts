import { computed, onMounted, ref, watch, type Ref } from 'vue';
import { useLoadingAfterMotion } from './useFirstLoad';
import { open as showOpenDialog } from '@tauri-apps/plugin-dialog';
import { useSyncController } from './useSyncController';
import { isTauri, tauriApi, toUserMessage } from './useTauriApi';
import { createLoadSeq } from '../lib/loadSeq';
import { workoutPageQueries } from '../lib/pageQueries';
import { cached, peekAll } from '../lib/readCache';
import { afterMotion } from '../lib/motion/budget';
import { workoutLabel } from '../lib/labels';
import { readDefaultExportFormat } from '../lib/exportScope';
import { workoutDisplayType } from '../lib/workouts';
import { intlLocale, useMessages } from '../i18n';
import { workoutDetailMessages } from '../views/WorkoutDetail.i18n';
import type { DeviceProfile, SportOption, Workout, WorkoutInsight, WorkoutSeries, WorkoutSeriesSample } from '../types';

export type WorkoutMetrics = Workout & {
  pace?: number | string | null;
  duration_minutes?: number | null;
};
export type ExportFormat = 'json' | 'csv' | 'gpx' | 'fit';

export const useWorkoutDetail = (workoutId: Ref<string>) => {
  const t = useMessages(workoutDetailMessages);
  const { dataRevision, markDataChanged } = useSyncController();
  const workout = ref<WorkoutMetrics | null>(null);
  const series = ref<WorkoutSeries | null>(null);
  const device = ref<DeviceProfile>({});
  const loading = ref(true);
  const preloaded = isTauri() && workoutId.value ? peekAll(workoutPageQueries(workoutId.value)) : null;
  if (preloaded) {
    workout.value = preloaded[0] as WorkoutMetrics | null;
    series.value = preloaded[1];
    device.value = preloaded[2];
    loading.value = false;
  }
  const error = ref<string | null>(null);
  const actionError = ref<string | null>(null);
  const exportedNote = ref<string | null>(null);
  const activeFormat = ref<ExportFormat>(readDefaultExportFormat());
  const exportBusy = ref(false);
  const displayType = computed(() => workout.value ? workoutDisplayType(workout.value) : 'unknown');
  const insight = ref<WorkoutInsight | null>(null);
  const insightLoading = ref(false);
  const insightError = ref<string | null>(null);
  const seriesError = ref<string | null>(null);
  const insightSeq = createLoadSeq();

  const loadInsight = async (id: string) => {
    const seq = insightSeq.next();
    if (!id) return;
    if (!isTauri()) {
      if (!insightSeq.isCurrent(seq)) return;
      insight.value = null;
      insightError.value = null;
      insightLoading.value = false;
      return;
    }
    insightLoading.value = true;
    insightError.value = null;
    try {
      const next = await tauriApi.getWorkoutInsight(id);
      if (!insightSeq.isCurrent(seq)) return;
      insight.value = next;
    } catch (cause) {
      if (!insightSeq.isCurrent(seq)) return;
      insight.value = null;
      insightError.value = toUserMessage(cause, t.value.insightFailed);
    } finally {
      if (insightSeq.isCurrent(seq)) insightLoading.value = false;
    }
  };

  const typeOverrideBusy = ref(false);
  const typeOverrideOptions = ref<SportOption[]>([]);
  const typeOverrideChoices = computed(() => [
    { value: '', label: t.value.noCorrection },
    ...typeOverrideOptions.value
      .map((option) => ({ value: option.key, label: workoutLabel(option.key) }))
      .sort((a, b) => a.label.localeCompare(b.label, intlLocale())),
  ]);

  let detailSeq = 0;
  const loadDetail = async () => {
    const seq = ++detailSeq;
    if (workout.value?.workout_id !== workoutId.value) loading.value = true;
    error.value = null;
    seriesError.value = null;
    if (!isTauri()) { loading.value = false; return; }
    try {
      const emptySeries: WorkoutSeries = { workout_id: workoutId.value, samples: [], route: [], pauses: [], splits: [], laps: [], summary: {} };
      const [detailQuery, seriesQuery, deviceQuery] = workoutPageQueries(workoutId.value);
      const [detail, seriesResult, profile] = await Promise.all([
        cached(detailQuery),
        cached(seriesQuery).then(
          (value) => ({ ok: true as const, value }),
          (cause) => ({ ok: false as const, cause }),
        ),
        cached(deviceQuery).catch(() => ({})),
      ]);
      if (seq !== detailSeq) return;
      workout.value = detail as WorkoutMetrics | null;
      if (!detail) {
        series.value = null;
        seriesError.value = null;
      } else if (seriesResult.ok) {
        series.value = seriesResult.value;
        seriesError.value = null;
      } else {
        series.value = emptySeries;
        seriesError.value = toUserMessage(seriesResult.cause, t.value.seriesFailed);
      }
      device.value = profile;
    } catch (cause) {
      if (seq === detailSeq) error.value = toUserMessage(cause, t.value.loadFailed);
    } finally {
      if (seq === detailSeq) loading.value = false;
    }
  };

  const changeWorkoutOverride = async (value: string | number) => {
    if (!workout.value) return;
    const id = workout.value.workout_id;
    const next = String(value);
    typeOverrideBusy.value = true;
    actionError.value = null;
    try {
      const updated = await tauriApi.setWorkoutTypeOverride(id, next || null);
      markDataChanged();
      if (id !== workoutId.value) return;
      workout.value = updated as WorkoutMetrics;
      exportedNote.value = next ? t.value.overrideSaved : t.value.overrideCleared;
      void loadInsight(updated.workout_id);
    } catch (cause) {
      if (id === workoutId.value) actionError.value = toUserMessage(cause, t.value.overrideFailed);
    } finally {
      typeOverrideBusy.value = false;
    }
  };

  const exportRecord = async () => {
    if (!workout.value || exportBusy.value) return;
    const exportWorkoutId = workout.value.workout_id;
    actionError.value = null;
    exportedNote.value = null;
    if (seriesError.value && activeFormat.value !== 'fit') {
      actionError.value = t.value.exportNeedsSeries;
      return;
    }
    exportBusy.value = true;
    try {
      if (activeFormat.value === 'fit') {
        const path = await showOpenDialog({ title: t.value.saveFit, directory: true, multiple: false });
        if (!path || typeof path !== 'string') return;
        await tauriApi.saveFitExport({
          scope: { kind: 'workout', workoutId: exportWorkoutId },
          dataTypes: ['workouts'],
          detail: 'full',
        }, path);
        exportedNote.value = t.value.savedFit;
        return;
      }
      if (activeFormat.value === 'gpx') {
        await tauriApi.copyWorkoutGpx(exportWorkoutId);
        exportedNote.value = t.value.copied('GPX');
        return;
      }
      const csvCell = (value: unknown): string => `"${String(value ?? '').replace(/"/g, '""')}"`;
      const csv = () => {
        const fields: Array<keyof WorkoutSeriesSample> = ['timestamp', 'heart_rate', 'speed', 'pace', 'cadence', 'stride_cm', 'altitude_m', 'power_watts', 'ground_contact_ms', 'vertical_oscillation_mm', 'vertical_ratio_pct', 'equivalent_pace_s_per_km'];
        return [fields.join(','), ...(series.value?.samples ?? []).map((sample) => fields.map((field) => csvCell(sample[field])).join(','))].join('\r\n');
      };
      const payload = activeFormat.value === 'json'
        ? JSON.stringify({ workout: workout.value, series: series.value }, null, 2)
        : csv();
      await navigator.clipboard.writeText(payload);
      exportedNote.value = t.value.copied(activeFormat.value.toUpperCase());
    } catch (cause) { actionError.value = toUserMessage(cause, t.value.exportFailed); }
    finally { exportBusy.value = false; }
  };

  onMounted(() => {
    if (preloaded) afterMotion(() => { void loadDetail(); });
    else void loadDetail();
    if (isTauri()) {
      void tauriApi.getWorkoutTypeOptions()
        .then((options) => { typeOverrideOptions.value = options; })
        .catch(() => { typeOverrideOptions.value = []; });
    }
  });
  watch([dataRevision, workoutId], () => void loadDetail());
  watch(workoutId, (id) => { if (id) void loadInsight(id); }, { immediate: true });

  return {
    workout, series, device, loading: useLoadingAfterMotion(loading), error, actionError, exportedNote, activeFormat, exportBusy, displayType,
    insight, insightLoading, insightError, seriesError,
    typeOverrideBusy, typeOverrideChoices, changeWorkoutOverride,
    loadDetail, exportRecord,
  };
};
