import { computed, ref, watch } from 'vue';
import { useSyncController } from '../useSyncController';
import { backend, toUserMessage } from '../../lib/bridge';
import { useMessages } from '../../i18n';
import { backendText } from '../../i18n/backendText';
import { settingsMessages } from '../../views/Settings.i18n';
import type { CapabilityItem, CapabilityOverview, CapabilityProbe } from '../../types';

const lookup = (table: unknown, key: string): string | undefined =>
  (table as Record<string, string | undefined>)[key];

const surfaceLabels: Record<string, string> = {
  v2_events: '/v2/users/me/events',
  user_events: '/users/{id}/events',
  user_events_day: '/users/{id}/events/dateString',
  file_info_events: '/users/me/fileInfo/events',
  blood_pressure_days: '/users/me/bloodPressure (days/toDate)',
  blood_pressure_range: '/users/me/bloodPressure (from/to)',
};

export const useCapabilityBoard = () => {
  const t = useMessages(settingsMessages);
  const { syncState } = useSyncController();
  const capabilityOverview = ref<CapabilityOverview | null>(null);
  const capabilityError = ref<string | null>(null);
  const probeBusy = ref(false);
  const probeResults = ref<CapabilityProbe[] | null>(null);

  const streamLabel = (stream: string): string => lookup(t.value.stream, stream) ?? stream;

  const unitLabel = (item: CapabilityItem): string =>
    (item.recordsUnitCode === 'days' ? t.value.unitDays : t.value.unitRecords);

  const capabilityNote = (item: CapabilityItem): string => {
    const windowDays = item.windowDays ?? 0;
    if (item.status === 'available' && item.ingested === false) {
      return item.stream === 'food' ? t.value.capabilityFoodHistoryHint : t.value.capabilityNotIngested;
    }
    if (item.status === 'unsupported') return t.value.capabilityUnsupported;
    if (item.status === 'unknown') return item.source === 'probed' ? t.value.capabilityUnconfirmed : t.value.capabilityNotProbed;
    if (item.status === 'no_records') {
      return item.source === 'probed'
        ? t.value.capabilityNoneProbed(windowDays)
        : t.value.capabilityNoRecords(windowDays);
    }
    return backendText(item.note, '');
  };

  const capabilityRow = (item: CapabilityItem) => ({
    key: item.stream,
    label: streamLabel(item.stream),
    detail:
      item.status === 'available' && item.ingested !== false
        ? t.value.capabilityLocal(item.records, unitLabel(item), item.latestDate ?? '')
        : item.status === 'available'
          ? t.value.capabilityCloud(item.records, unitLabel(item), item.latestDate ?? '')
          : capabilityNote(item),
    note: item.ingested === false ? capabilityNote(item) : null,
  });

  const capabilityAvailable = computed(() =>
    (capabilityOverview.value?.items ?? [])
      .filter((item) => item.status === 'available' && item.ingested !== false)
      .map(capabilityRow),
  );

  const capabilityNotIngested = computed(() =>
    (capabilityOverview.value?.items ?? [])
      .filter((item) => item.status === 'available' && item.ingested === false)
      .map(capabilityRow),
  );

  const capabilityMissing = computed(() =>
    (capabilityOverview.value?.items ?? [])
      .filter((item) => item.status !== 'available')
      .map(capabilityRow),
  );

  const capabilityBoard = computed(() => [
    ...capabilityAvailable.value.map((row) => ({ ...row, state: 'on', lamp: 'on', note: undefined as string | undefined })),
    ...capabilityNotIngested.value.map((row) => ({ ...row, state: 'pending', lamp: 'pending' })),
    ...capabilityMissing.value.map((row) => ({ ...row, state: 'off', lamp: 'off', note: undefined as string | undefined })),
  ]);

  const capabilityCheckedAt = computed(() => {
    const raw = capabilityOverview.value?.probedAt;
    if (!raw) return null;
    const then = new Date(raw).getTime();
    if (!Number.isFinite(then)) return null;
    const days = Math.floor((Date.now() - then) / 86400000);
    return days <= 0 ? t.value.probedToday : t.value.probedDaysAgo(days);
  });

  const loadCapabilityOverview = async () => {
    capabilityError.value = null;
    try {
      capabilityOverview.value = await backend.getCapabilityOverview();
    } catch (error) {
      capabilityError.value = toUserMessage(error);
    }
  };

  watch(syncState, (current, previous) => {
    if (previous === 'syncing' && current !== 'syncing') void loadCapabilityOverview();
  });

  /** One line per probed endpoint — for diagnosing, not for reading. */
  const probeDiagnostics = computed(() => {
    if (!probeResults.value) return [];
    return probeResults.value.map((probe) => {
      const name = `${probe.eventType}${probe.subType ? '/' + probe.subType : ''}`;
      const surface = surfaceLabels[probe.surface] ?? probe.surface;
      const result =
        probe.status === 'available'
          ? t.value.probeRecords(probe.records, probe.latestDate ?? '')
          : probe.status === 'empty'
            ? t.value.probeEmpty
            : probe.status === 'unavailable'
              ? t.value.probeRefused
              : probe.status === 'unrecognized'
                ? t.value.probeUnrecognized
                : t.value.probeFailed;
      return `${name} @ ${surface} — ${result}${probe.failure ? ` (${probe.failure})` : ''} · ${t.value.probeWindow(probe.windowDays)}${probe.fields.length ? ` · ${probe.fields.join(', ')}` : ''}`;
    });
  });

  const runCapabilityProbe = async () => {
    probeBusy.value = true;
    capabilityError.value = null;
    try {
      probeResults.value = await backend.probeDataCapabilities();
      await loadCapabilityOverview();
    } catch (error) {
      capabilityError.value = toUserMessage(error);
    } finally {
      probeBusy.value = false;
    }
  };

  return {
    capabilityOverview,
    capabilityError,
    probeBusy,
    capabilityAvailable,
    capabilityNotIngested,
    capabilityMissing,
    capabilityBoard,
    capabilityCheckedAt,
    probeDiagnostics,
    loadCapabilityOverview,
    runCapabilityProbe,
  };
};
