import { computed, getCurrentInstance, onActivated, onMounted, ref, shallowRef } from 'vue';
import router from '../router';
import { TOUR_STEPS, type SectionId, type TourChapterId, type TourStep } from '../lib/tour/chapters';
import { chaptersToMark, presentSteps, type TourEnd } from '../lib/tour/progress';
import {
  enterSandbox, introPending, leaveSandbox, markSeen, sandboxFlags, seenChapters,
} from '../lib/tour/sandbox';
import { updateNotice } from '../services/updateNotice';
import { locale } from '../i18n';
import { useTheme } from './useTheme';
import { lessMotion } from '../lib/motion/lessMotion';

export type TourMode = 'sandbox' | 'inline';

interface ActiveTour {
  chapters: TourChapterId[];
  keys: number[][];
  chapter: number;
  step: number;
  mode: TourMode;
  path: string;
  keepUnseen?: boolean;
  onDone?: (end: TourEnd) => void;
}

export interface TargetBox { x: number; y: number; width: number; height: number }

const active = shallowRef<ActiveTour | null>(null);
const box = shallowRef<TargetBox | null>(null);
const settling = ref(false);
const welcomeOpen = ref(false);

const WAIT_MS = 4000;
const INLINE_WAIT_MS = 1200;
const OFFER_WAIT_MS = 3000;
const FALLBACK_WAIT_MS = 900;
const STEADY_FRAMES = 6;
const STEADY_MAX_MS = 1600;
const PAD = 8;
let token = 0;
let targets: Element[] = [];
let selector: { css: string; all: boolean } | null = null;
let refinding = false;
let measureFrame = 0;
let direction: 1 | -1 = 1;
let shownSteps = 0;

const reducedMotion = () => lessMotion();
const sleep = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

const findTargets = (css: string, all: boolean, run: number, wait = WAIT_MS): Promise<Element[]> => new Promise((resolve) => {
  const started = performance.now();
  const tick = () => {
    if (run !== token) return resolve([]);
    const visible = [...document.querySelectorAll(css)]
      .filter((element) => element.getClientRects().length > 0 && !element.closest('[class*="-leave-active"]'));
    const found = all ? visible : visible.slice(0, 1);
    if (found.length || performance.now() - started > wait) return resolve(found);
    window.setTimeout(tick, 80);
  };
  tick();
});

const boxOf = (elements: Element[]): TargetBox | null => {
  const rects = elements.map((element) => element.getBoundingClientRect()).filter((r) => r.width || r.height);
  if (!rects.length) return null;
  const rect = {
    left: Math.min(...rects.map((r) => r.left)), top: Math.min(...rects.map((r) => r.top)),
    right: Math.max(...rects.map((r) => r.right)), bottom: Math.max(...rects.map((r) => r.bottom)),
  };
  const x = Math.max(rect.left - PAD, 4);
  const y = Math.max(rect.top - PAD, 4);
  const right = Math.min(rect.right + PAD, window.innerWidth - 4);
  const bottom = Math.min(rect.bottom + PAD, window.innerHeight - 4);
  return { x, y, width: Math.max(right - x, 0), height: Math.max(bottom - y, 0) };
};
const sameBox = (a: TargetBox | null, b: TargetBox | null) => !!a && !!b
  && Math.abs(a.x - b.x) < 0.5 && Math.abs(a.y - b.y) < 0.5 && Math.abs(a.width - b.width) < 0.5 && Math.abs(a.height - b.height) < 0.5;

const waitSteady = (elements: Element[], run: number): Promise<void> => new Promise((resolve) => {
  const started = performance.now();
  let last: TargetBox | null = null;
  let still = 0;
  const tick = () => {
    if (run !== token) return resolve();
    const now = boxOf(elements.filter((element) => element.isConnected));
    still = sameBox(now, last) ? still + 1 : 0;
    last = now;
    if (still >= STEADY_FRAMES || performance.now() - started > STEADY_MAX_MS) return resolve();
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
});

const measure = () => {
  measureFrame = 0;
  const live = targets.filter((element) => element.isConnected);
  if (!live.length) {
    if (refinding || !active.value || settling.value || !selector) return;
    refinding = true;
    const run = token;
    const { css, all } = selector;
    void findTargets(css, all, run, 600).then((found) => {
      refinding = false;
      if (run !== token) return;
      if (found.length) { targets = found; scheduleMeasure(); return; }
      targets = [];
      void show();
    });
    return;
  }
  const next = boxOf(live);
  if (next && !sameBox(next, box.value)) box.value = next;
};
function scheduleMeasure() {
  if (!measureFrame && (targets.length || selector)) measureFrame = requestAnimationFrame(measure);
}

const listen = (on: boolean) => {
  const method = on ? 'addEventListener' : 'removeEventListener';
  window[method]('resize', scheduleMeasure);
  document[method]('scroll', scheduleMeasure, { capture: true, passive: true } as AddEventListenerOptions);
};

const keyOf = (tour: ActiveTour): number | undefined => tour.keys[tour.chapter]?.[tour.step];
const stepOf = (tour: ActiveTour): TourStep | undefined => {
  const key = keyOf(tour);
  return key === undefined ? undefined : TOUR_STEPS[tour.chapters[tour.chapter]!]?.[key];
};

let openedTable = false;
const closeOpened = async () => {
  if (!openedTable) return;
  openedTable = false;
  const close = document.querySelector<HTMLElement>('.card-table [data-table-close]');
  if (!close) return;
  close.click();
  await sleep(reducedMotion() ? 120 : 620);
};
const openPickDays = async (run: number) => {
  if (document.querySelector('.card-table')) { openedTable = true; return; }
  const [button] = await findTargets('.pick-days', false, run);
  if (run !== token || !(button instanceof HTMLElement)) return;
  button.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'auto' });
  button.click();
  openedTable = true;
};

async function show() {
  const tour = active.value;
  const step = tour && stepOf(tour);
  if (!tour || !step) return;
  const run = ++token;
  settling.value = true;
  targets = [];
  selector = null;
  const sandbox = tour.mode === 'sandbox';
  if (!step.open) await closeOpened();
  if (run !== token) return;
  if (sandbox && step.route && router.currentRoute.value.path !== step.route) await router.push(step.route).catch(() => undefined);
  if (run !== token) return;
  if (step.open === 'pickDays') await openPickDays(run);
  let css = step.target;
  const wait = !sandbox ? INLINE_WAIT_MS : step.fallback ? FALLBACK_WAIT_MS : WAIT_MS;
  let found = css ? await findTargets(css, step.all === true, run, wait) : [];
  if (run !== token) return;
  if (!found.length && step.fallback && sandbox) {
    if (router.currentRoute.value.path !== step.fallback.route) await router.push(step.fallback.route).catch(() => undefined);
    css = step.fallback.target;
    found = await findTargets(css, false, run);
    if (run !== token) return;
  }
  if (found.length && css) {
    found[0]!.scrollIntoView({ block: 'center', inline: 'nearest', behavior: reducedMotion() ? 'auto' : 'smooth' });
    await sleep(reducedMotion() ? 0 : 200);
    if (run !== token) return;
    await waitSteady(found, run);
    if (run !== token) return;
    if (step.all) {
      const again = await findTargets(css, true, run, 0);
      if (run !== token) return;
      if (again.length) found = again;
    }
    targets = found;
    selector = { css, all: step.all === true };
    measure();
  } else if (css) {
    skipMissing();
    return;
  } else {
    box.value = null;
  }
  shownSteps += 1;
  settling.value = false;
}

function skipMissing() {
  const tour = active.value;
  if (!tour) return;
  const atStart = tour.chapter === 0 && tour.step === 0;
  move(direction === -1 && !atStart ? -1 : 1);
}

const finish = (end: TourEnd) => {
  const tour = active.value;
  if (!tour) return;
  if (!tour.keepUnseen) markSeen(chaptersToMark(end, tour.chapters, tour.chapter));
  token += 1;
  targets = [];
  selector = null;
  box.value = null;
  active.value = null;
  settling.value = false;
  listen(false);
  void closeOpened();
  tour.onDone?.(end);
  if (tour.mode === 'sandbox') leaveSandbox(sandboxFlags()?.ret ?? '/');
};

router.afterEach((to) => {
  const tour = active.value;
  if (tour?.mode === 'inline' && to.path !== tour.path) finish('pause');
});

const begin = (
  chapters: TourChapterId[],
  keys: number[][],
  mode: TourMode,
  options: { keepUnseen?: boolean; onDone?: (end: TourEnd) => void } = {},
) => {
  if (!chapters.length) return;
  if (!active.value) listen(true);
  direction = 1;
  shownSteps = 0;
  active.value = {
    chapters,
    keys,
    chapter: 0,
    step: 0,
    mode,
    path: router.currentRoute.value.path,
    keepUnseen: options.keepUnseen,
    onDone: options.onDone,
  };
  void show();
};

export const startTour = (chapters: TourChapterId[], mode: TourMode): void => {
  const list = chapters.filter((id) => TOUR_STEPS[id]?.length);
  begin(list, list.map((id) => TOUR_STEPS[id].map((_, index) => index)), mode);
};

export const launchSandbox = (chapters: TourChapterId[], options: { welcome?: boolean; ret?: string } = {}): void => {
  const first = chapters.map((id) => TOUR_STEPS[id]?.[0]?.route).find(Boolean);
  enterSandbox({
    chapters,
    welcome: options.welcome,
    ret: options.ret ?? router.currentRoute.value.fullPath,
    route: first ?? '/',
    theme: useTheme().themeMode.value,
    locale: locale.value,
  });
};

const isVisible = (css: string) => [...document.querySelectorAll(css)]
  .some((element) => element.getClientRects().length > 0 && !element.closest('[class*="-leave-active"]'));

let offering = 0;
export const offerInline = async (chapter: SectionId): Promise<void> => {
  if (active.value || window.__ZB_DEMO__?.demo || welcomeOpen.value) return;
  if (updateNotice.receipt || introPending() || seenChapters().has(chapter)) return;
  const run = ++offering;
  const path = router.currentRoute.value.path;
  const steps = TOUR_STEPS[chapter];
  const started = performance.now();
  let keys = presentSteps(steps, isVisible);
  while (!keys.length && performance.now() - started < OFFER_WAIT_MS) {
    await sleep(120);
    if (run !== offering || router.currentRoute.value.path !== path) return;
    keys = presentSteps(steps, isVisible);
  }
  if (!keys.length || active.value || router.currentRoute.value.path !== path) return;
  begin([chapter], [keys], 'inline');
};

export const offerInlineSoon = (chapter: SectionId, delay: number, stillThere: () => boolean = () => true): void => {
  const instance = getCurrentInstance();
  window.setTimeout(() => {
    if (instance && (instance.isUnmounted || instance.isDeactivated)) return;
    if (stillThere()) void offerInline(chapter);
  }, delay);
};

export const useSectionTour = (chapter: SectionId, stillThere?: () => boolean, delay = 900): void => {
  const offer = () => offerInlineSoon(chapter, delay, stillThere);
  onMounted(offer);
  onActivated(offer);
};

function move(delta: 1 | -1) {
  const tour = active.value;
  if (!tour) return;
  direction = delta;
  let { chapter, step } = tour;
  step += delta;
  if (step >= tour.keys[chapter]!.length) { chapter += 1; step = 0; }
  if (step < 0) {
    if (chapter === 0) return;
    chapter -= 1;
    step = tour.keys[chapter]!.length - 1;
  }
  if (chapter >= tour.chapters.length) { finish(shownSteps ? 'done' : 'abort'); return; }
  active.value = { ...tour, chapter, step };
  void show();
}

export const useTour = () => ({
  active,
  box,
  settling,
  welcomeOpen,
  current: computed(() => {
    const tour = active.value;
    if (!tour) return null;
    const keys = tour.keys[tour.chapter]!;
    const total = tour.keys.reduce((sum, list) => sum + list.length, 0);
    const done = tour.keys.slice(0, tour.chapter).reduce((sum, list) => sum + list.length, 0) + tour.step;
    return {
      chapter: tour.chapters[tour.chapter]!,
      step: keys[tour.step]!,
      stepsInChapter: keys.length,
      index: done,
      total,
      first: done === 0,
      last: done === total - 1,
      mode: tour.mode,
      interactive: stepOf(tour)?.interactive === true,
    };
  }),
  next: () => move(1),
  prev: () => move(-1),
  skip: () => finish('skip'),
});
