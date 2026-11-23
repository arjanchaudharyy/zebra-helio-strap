import { computed, readonly, ref } from 'vue';
import { isDesktop } from '../lib/bridge';
import { lessMotion } from '../lib/motion/lessMotion';
import { captureZoomGhost, type ZoomGhost } from '../lib/motion/zoomGhost';
import { sameZoom, scaleFromTransform, ZOOM_COMMIT_TIMEOUT_MS, ZOOM_EASE, zoomLeg } from '../lib/uiZoom';

export const UI_SCALES = [80, 90, 100, 110, 125] as const;
export type UiScale = (typeof UI_SCALES)[number];

export const DEFAULT_UI_SCALE: UiScale = 100;

const STORAGE_KEY = 'zeppbridge-ui-scale';
const scale = ref<UiScale>(DEFAULT_UI_SCALE);
let initialized = false;

const isUiScale = (value: number): value is UiScale =>
  (UI_SCALES as readonly number[]).includes(value);

const readScale = (): UiScale => {
  if (typeof window === 'undefined') return DEFAULT_UI_SCALE;
  const saved = Number(window.localStorage.getItem(STORAGE_KEY));
  return isUiScale(saved) ? saved : DEFAULT_UI_SCALE;
};

const applyCssZoom = (factor: number) => {
  const root = document.documentElement;
  root.style.zoom = factor === 1 ? '' : String(factor);
  root.style.setProperty('--ui-scale', String(factor));
};

let nativeBroken = false;
const useNative = () => isDesktop() && !nativeBroken;

const applyFactor = (factor: number, done: () => void = () => undefined) => {
  if (!useNative()) {
    applyCssZoom(factor);
    done();
    return;
  }
  applyCssZoom(1);
  void import('@tauri-apps/api/webview')
    .then(({ getCurrentWebview }) => getCurrentWebview().setZoom(factor))
    .catch(() => {
      nativeBroken = true;
      applyCssZoom(factor);
      done();
    });
};

let applied = 1;
let leg: Animation | null = null;
let committing = false;
let queued: number | null = null;
let ghost: ZoomGhost | null = null;

const clearBody = () => {
  const style = document.body?.style;
  if (!style) return;
  style.removeProperty('transform-origin');
  style.removeProperty('will-change');
};

const cancelLeg = () => {
  leg?.cancel();
  leg = null;
  clearBody();
};

const whenZoomLands = (land: () => void) => {
  const width = window.innerWidth;
  const height = window.innerHeight;
  const dpr = window.devicePixelRatio;
  let landed = false;
  let timer = 0;
  const finish = () => {
    if (landed) return;
    landed = true;
    window.removeEventListener('resize', onResize);
    window.clearTimeout(timer);
    land();
  };
  const onResize = () => {
    if (window.innerWidth !== width || window.innerHeight !== height || window.devicePixelRatio !== dpr) finish();
  };
  window.addEventListener('resize', onResize);
  timer = window.setTimeout(finish, ZOOM_COMMIT_TIMEOUT_MS);
  return finish;
};

const commit = (factor: number) => {
  if (sameZoom(factor, applied)) {
    cancelLeg();
    return;
  }
  committing = true;
  ghost?.dispose();
  const shot = captureZoomGhost(factor / applied);
  let settled = false;
  const land = () => {
    if (settled) return;
    settled = true;
    cancelLeg();
    applied = factor;
    committing = false;
    ghost = shot;
    shot?.reveal();
    if (queued !== null) {
      const next = queued;
      queued = null;
      transition(next);
    }
  };
  if (useNative()) {
    const landNow = whenZoomLands(land);
    applyFactor(factor, landNow);
  } else {
    applyFactor(factor, land);
  }
};

function transition(factor: number) {
  if (committing) {
    queued = factor;
    return;
  }
  const body = document.body;
  if (!body || typeof body.animate !== 'function' || lessMotion()) {
    cancelLeg();
    ghost?.dispose();
    ghost = null;
    if (!sameZoom(factor, applied)) {
      applied = factor;
      applyFactor(factor);
    }
    return;
  }
  const current = leg ? scaleFromTransform(getComputedStyle(body).transform) : 1;
  leg?.cancel();
  leg = null;
  const plan = zoomLeg(applied, current, factor);
  if (plan.durationMs === 0) {
    commit(factor);
    return;
  }
  body.style.transformOrigin = '50% 0';
  body.style.willChange = 'transform';
  const animation = body.animate(
    [{ transform: `scale(${plan.from})` }, { transform: `scale(${plan.to})` }],
    { duration: plan.durationMs, easing: ZOOM_EASE, fill: 'forwards' },
  );
  leg = animation;
  animation.finished.then(
    () => { if (leg === animation) commit(factor); },
    () => undefined,
  );
}

const applyScale = (value: UiScale, animate: boolean) => {
  if (typeof document === 'undefined') return;
  const factor = value / 100;
  if (animate) {
    transition(factor);
    return;
  }
  cancelLeg();
  applied = factor;
  applyFactor(factor);
};

const setScale = (value: UiScale) => {
  scale.value = value;
  if (typeof window !== 'undefined') window.localStorage.setItem(STORAGE_KEY, String(value));
  applyScale(value, true);
};

const bumpScale = (direction: 1 | -1) => {
  const index = UI_SCALES.indexOf(scale.value);
  const next = UI_SCALES[Math.min(UI_SCALES.length - 1, Math.max(0, index + direction))];
  if (next !== scale.value) setScale(next);
};

const resetScale = () => {
  if (scale.value !== DEFAULT_UI_SCALE) setScale(DEFAULT_UI_SCALE);
};

const initializeScale = () => {
  if (initialized) return;
  initialized = true;
  scale.value = readScale();
  applyScale(scale.value, false);
};

export const useUiScale = () => ({
  scale: readonly(scale),
  scaleLabel: computed(() => `${scale.value}%`),
  initializeScale,
  setScale,
  bumpScale,
  resetScale,
});
