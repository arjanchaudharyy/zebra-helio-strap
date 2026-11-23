import { computed, ref } from 'vue';
import { useHistoryBackfill } from './useHistoryBackfill';
import { useSyncController } from './useSyncController';
import { backend, isDesktop } from '../lib/bridge';
import type { BackfillChoice } from '../lib/backfillRange';
import type { UserPrefs } from '../types';

export type OfferYears = 1 | 2 | 3 | 10;
const CHOICE: Record<OfferYears, BackfillChoice> = { 1: '1y', 2: '2y', 3: '3y', 10: 'all' };
const SLACK_DAYS = 7;

const dismissKey = (place: string) => `zb.backfillOffer.dismissed.${place}`;
const readDismissed = (place: string): boolean => {
  try { return window.localStorage.getItem(dismissKey(place)) === '1'; } catch { return false; }
};

const dayNumber = (day: string) => Date.parse(`${day}T00:00:00Z`) / 86_400_000;

export const useBackfillOffer = (place: string) => {
  const { appStatus } = useSyncController();
  const prefs = ref<UserPrefs | null>(null);
  const backfill = useHistoryBackfill(() => prefs.value, (next) => { prefs.value = next; });
  const dismissed = ref(readDismissed(place));
  const earliest = computed(() => appStatus.value?.coverage?.earliest_day ?? null);

  const shortOf = (start: string): boolean => {
    const first = earliest.value;
    if (!isDesktop() || !first || dayNumber(first) - dayNumber(start) <= SLACK_DAYS) return false;
    const asked = backfill.ledger.value?.requested_from;
    return !asked || asked > start;
  };

  const dismiss = () => {
    dismissed.value = true;
    try { window.localStorage.setItem(dismissKey(place), '1'); } catch { }
  };

  const start = async (years: OfferYears) => {
    if (backfill.busy.value) return;
    if (!prefs.value) {
      try { prefs.value = await backend.getUserPrefs(); } catch { prefs.value = null; }
    }
    backfill.startChoice.value = CHOICE[years];
    void backfill.runBackfill();
  };

  return {
    earliest, dismissed, shortOf, dismiss, start,
    busy: backfill.busy, error: backfill.error, message: backfill.message, stop: backfill.stopBackfill,
  };
};
