import { computed, ref } from 'vue';

export type MotionTier = 'high' | 'mid' | 'low';
export const ORDER: MotionTier[] = ['low', 'mid', 'high'];
const TIER_KEY = 'zeppbridge.motion.tier.v1';
export const LOW_BATTERY = 0.15;

const lower = (a: MotionTier, b: MotionTier): MotionTier => (ORDER.indexOf(a) <= ORDER.indexOf(b) ? a : b);
export const down = (tier: MotionTier): MotionTier => ORDER[Math.max(0, ORDER.indexOf(tier) - 1)]!;

export const tierFromCores = (cores?: number): MotionTier =>
  !cores ? 'mid' : cores >= 8 ? 'high' : cores >= 4 ? 'mid' : 'low';

export const tierFromFrames = (intervals: number[], ceiling: MotionTier): MotionTier => {
  const usable = intervals.filter((ms) => ms > 0 && ms < 200).sort((a, b) => a - b);
  if (usable.length < 12) return ceiling;
  const p90 = usable[Math.floor(usable.length * 0.9)]!;
  return lower(p90 > 40 ? 'low' : p90 > 22 ? 'mid' : 'high', ceiling);
};

export const effectWasJanky = (intervals: number[]): boolean => {
  const usable = intervals.filter((ms) => ms > 0 && ms < 200);
  return usable.length >= 12 && usable.filter((ms) => ms > 33).length / usable.length > 0.3;
};

export const readTier = (): MotionTier | null => {
  try {
    const saved = window.localStorage.getItem(TIER_KEY);
    return saved === 'high' || saved === 'mid' || saved === 'low' ? saved : null;
  } catch { return null; }
};
export const saveTier = (tier: MotionTier) => {
  try { window.localStorage.setItem(TIER_KEY, tier); } catch { }
};

export const cores = () => (typeof navigator === 'undefined' ? undefined : navigator.hardwareConcurrency);
export const motionTier = ref<MotionTier>((typeof window !== 'undefined' && readTier()) || tierFromCores(cores()));
export const power = ref({ onBattery: false, low: false });
export const autoLite = computed(() => motionTier.value === 'low' || power.value.low);

