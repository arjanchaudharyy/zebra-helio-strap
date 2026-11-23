import { computed, ref } from 'vue';

const holds = ref(0);
let waiters: Array<() => void> = [];

export const motionBusy = computed(() => holds.value > 0);

const mark = () => {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  if (holds.value > 0 || staging > 0) {
    if (root.dataset.motion !== 'busy') root.dataset.motion = 'busy';
  } else if (root.dataset.motion) {
    delete root.dataset.motion;
  }
};

const flush = () => {
  if (holds.value > 0) return;
  const ready = waiters;
  waiters = [];
  for (const resolve of ready) resolve();
};

export const holdMotion = (maxMs = 1200): (() => void) => {
  holds.value += 1;
  mark();
  let released = false;
  const release = () => {
    if (released) return;
    released = true;
    clearTimeout(timer);
    holds.value = Math.max(0, holds.value - 1);
    mark();
    flush();
  };
  const timer = setTimeout(release, maxMs);
  return release;
};

export const whenMotionIdle = (): Promise<void> =>
  holds.value > 0 ? new Promise((resolve) => { waiters.push(resolve); }) : Promise.resolve();

let staging = 0;
let stagingWaiters: Array<() => void> = [];
export const stageMotion = (maxMs = 600): (() => void) => {
  staging += 1;
  mark();
  let released = false;
  const release = () => {
    if (released) return;
    released = true;
    clearTimeout(timer);
    staging = Math.max(0, staging - 1);
    mark();
    if (staging > 0) return;
    const ready = stagingWaiters;
    stagingWaiters = [];
    for (const resolve of ready) resolve();
  };
  const timer = setTimeout(release, maxMs);
  return release;
};
const whenStaged = (): Promise<void> =>
  staging > 0 ? new Promise((resolve) => { stagingWaiters.push(resolve); }) : Promise.resolve();

export const afterMotion = (run: () => void): void => {
  const frames = (left: number) => {
    if (left <= 0) {
      void whenStaged().then(whenMotionIdle).then(run);
      return;
    }
    requestAnimationFrame(() => frames(left - 1));
  };
  frames(3);
};

const slots: Array<() => void> = [];
let slotFrame = 0;
let slotWaiters: Array<() => void> = [];
const pumpSlots = () => {
  try {
    slots.shift()?.();
  } catch (error) {
    console.error(error);
  }
  if (slots.length) {
    slotFrame = requestAnimationFrame(pumpSlots);
    return;
  }
  slotFrame = 0;
  const ready = slotWaiters;
  slotWaiters = [];
  for (const resolve of ready) resolve();
};
export const nextSlot = (job: () => void): void => {
  slots.push(job);
  if (!slotFrame) slotFrame = requestAnimationFrame(pumpSlots);
};
export const whenSlotsDrained = (maxMs = 260): Promise<void> =>
  new Promise((resolve) => {
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      resolve();
    };
    const timer = setTimeout(finish, maxMs);
    requestAnimationFrame(() => {
      if (slots.length) slotWaiters.push(finish);
      else finish();
    });
  });

let resizeHolds = 0;
const resizeJobs = new Set<() => void>();
export const holdResizes = (maxMs = 900): (() => void) => {
  resizeHolds += 1;
  let released = false;
  const release = () => {
    if (released) return;
    released = true;
    clearTimeout(timer);
    resizeHolds = Math.max(0, resizeHolds - 1);
  };
  const timer = setTimeout(release, maxMs);
  return release;
};
export const deferResize = (job: () => void): boolean => {
  if (!resizeHolds) return false;
  if (!resizeJobs.has(job)) {
    resizeJobs.add(job);
    nextSlot(() => { resizeJobs.delete(job); job(); });
  }
  return true;
};

export const resetMotionBudget = () => {
  holds.value = 0;
  resizeHolds = 0;
  resizeJobs.clear();
  slots.length = 0;
  staging = 0;
  mark();
  if (slotFrame) cancelAnimationFrame(slotFrame);
  slotFrame = 0;
  slotWaiters = [];
  const ready = stagingWaiters;
  stagingWaiters = [];
  for (const resolve of ready) resolve();
  flush();
};
