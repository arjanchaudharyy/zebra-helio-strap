import { computed, ref } from 'vue';
import type { AiProviderId } from '../aiProviders';
import { formatNumber } from '../format';

export const FREE_TOKEN_BUDGET = 30_000;
export const SUBSCRIBED_TOKEN_BUDGET = 120_000;

const STORAGE_KEY = 'zeppbridge.ai.subscribed';

const read = (): Partial<Record<AiProviderId, boolean>> => {
  try {
    const parsed = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? '{}') as unknown;
    return parsed && typeof parsed === 'object' ? parsed as Partial<Record<AiProviderId, boolean>> : {};
  } catch {
    return {};
  }
};

const subscribed = ref(read());
export const currentProviderId = ref<AiProviderId | null>(null);

export const isSubscribed = (id: AiProviderId | null): boolean => Boolean(id && subscribed.value[id]);

export const setSubscribed = (id: AiProviderId, on: boolean) => {
  subscribed.value = { ...subscribed.value, [id]: on };
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(subscribed.value));
  } catch {
  }
};

export const tokenBudget = computed(() =>
  (isSubscribed(currentProviderId.value) ? SUBSCRIBED_TOKEN_BUDGET : FREE_TOKEN_BUDGET));

export const formatTokens = (tokens: number): string =>
  formatNumber(Math.max(0, tokens), { notation: 'compact', maximumFractionDigits: 1 });
