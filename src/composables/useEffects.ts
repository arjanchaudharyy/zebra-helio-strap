import { computed, readonly, ref, watch } from 'vue';
import { autoLite } from '../lib/motion/tier';

export type EffectsMode = 'auto' | 'full' | 'lite';

const STORAGE_KEY = 'zeppbridge-effects';

const isEffectsMode = (value: unknown): value is EffectsMode =>
  value === 'auto' || value === 'full' || value === 'lite';

const readMode = (): EffectsMode => {
  if (typeof window === 'undefined') return 'auto';
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (isEffectsMode(saved)) return saved;
  } catch {
  }
  return 'auto';
};

export const isLinuxWebView = (userAgent = typeof navigator === 'undefined' ? '' : navigator.userAgent): boolean =>
  /Linux/.test(userAgent) && !/Android/.test(userAgent);

export const resolveLite = (value: EffectsMode, linux: boolean, constrained = false): boolean =>
  value === 'lite' || (value === 'auto' && (linux || constrained));

const mode = ref<EffectsMode>(readMode());
const lite = computed(() => resolveLite(mode.value, isLinuxWebView(), autoLite.value));

const apply = () => {
  if (typeof document === 'undefined') return;
  if (lite.value) document.documentElement.dataset.effects = 'lite';
  else delete document.documentElement.dataset.effects;
};

const setEffects = (value: EffectsMode) => {
  mode.value = value;
  try {
    window.localStorage.setItem(STORAGE_KEY, value);
  } catch {
  }
  apply();
};

export const initializeEffects = () => {
  apply();
  watch(lite, apply);
};

export const useEffects = () => ({ mode: readonly(mode), lite, setEffects });
