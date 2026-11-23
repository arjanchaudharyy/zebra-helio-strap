import { ref } from 'vue';

const stamp = (date = new Date()): string => `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;

const day = ref(stamp());
let started = false;
let timer = 0;

const sync = () => {
  const next = stamp();
  if (next !== day.value) day.value = next;
};

const scheduleMidnight = () => {
  window.clearTimeout(timer);
  const now = new Date();
  const midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1, 0, 0, 1);
  timer = window.setTimeout(() => {
    sync();
    scheduleMidnight();
  }, Math.max(1_000, midnight.getTime() - now.getTime()));
};

const start = () => {
  if (started || typeof window === 'undefined') return;
  started = true;
  scheduleMidnight();
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState !== 'visible') return;
    sync();
    scheduleMidnight();
  });
};

export const today = (): Date => {
  start();
  void day.value;
  return new Date();
};
