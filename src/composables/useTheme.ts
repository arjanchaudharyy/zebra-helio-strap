import { computed, ref } from 'vue';
import { whenFramesSteady } from '../lib/motion/steady';
import { OPEN_EASE, REVEAL_MS } from '../lib/motion/timing';
import { canUseViewTransitions } from '../lib/motion/viewTransitions';
import { lessMotion } from '../lib/motion/lessMotion';

export type ThemeMode = 'light' | 'dark' | 'system';
export type ResolvedTheme = 'light' | 'dark';

const STORAGE_KEY = 'zeppbridge-theme';

const isThemeMode = (value: unknown): value is ThemeMode =>
  value === 'light' || value === 'dark' || value === 'system';

const readStoredMode = (): ThemeMode => {
  if (typeof window === 'undefined') return 'system';
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (isThemeMode(saved)) return saved;
  } catch {
  }
  return 'system';
};

const readSystemDark = (): boolean => {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return true;
  return window.matchMedia('(prefers-color-scheme: dark)').matches;
};

const mode = ref<ThemeMode>(readStoredMode());
const systemDark = ref(readSystemDark());

export const resolvedTheme = computed<ResolvedTheme>(() =>
  mode.value === 'system' ? (systemDark.value ? 'dark' : 'light') : mode.value);

export const systemTheme = computed<ResolvedTheme>(() => (systemDark.value ? 'dark' : 'light'));

export const pickTheme = (value: ThemeMode, origin?: ThemeOrigin) => {
  const target: ResolvedTheme = value === 'system' ? systemTheme.value : value;
  if (target === resolvedTheme.value) {
    setTheme(value);
    return;
  }
  revealTheme(() => setTheme(value), origin);
};

export type ThemeOrigin = { x: number; y: number };

type ViewTransitionHandle = { ready: Promise<void>; finished: Promise<void>; skipTransition: () => void };
type ViewTransitionDocument = Document & {
  startViewTransition?: (update: () => void) => ViewTransitionHandle;
};

let revealing: ViewTransitionHandle | null = null;

export const revealTheme = (update: () => void, origin?: ThemeOrigin) => {
  const doc = document as ViewTransitionDocument;
  const reduced = lessMotion();
  if (revealing) {
    revealing.skipTransition();
    revealing = null;
    update();
    return;
  }
  if (!canUseViewTransitions() || !doc.startViewTransition || reduced || !origin) {
    update();
    return;
  }
  const root = document.documentElement;
  const { innerWidth: width, innerHeight: height } = window;
  const reach = Math.ceil(Math.hypot(Math.max(origin.x, width - origin.x), Math.max(origin.y, height - origin.y)));
  const at = `at ${Math.round(origin.x)}px ${Math.round(origin.y)}px`;
  root.dataset.themeMorph = '';
  const transition = doc.startViewTransition(update);
  revealing = transition;
  void transition.ready.then(() => {
    const grow = root.animate(
      { clipPath: [`circle(2px ${at})`, `circle(${reach}px ${at})`] },
      { duration: REVEAL_MS, easing: OPEN_EASE, fill: 'both', pseudoElement: '::view-transition-new(root)' },
    );
    grow.pause();
    void whenFramesSteady().then(() => { if (grow.playState === 'paused') grow.play(); });
  }).catch(() => undefined);
  void transition.finished.catch(() => undefined).finally(() => {
    if (revealing === transition) revealing = null;
    delete root.dataset.themeMorph;
  });
};

const THEME_COLORS: Record<ResolvedTheme, string> = { dark: '#0D0F12', light: '#F4F6F3' };

const applyTheme = () => {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  root.dataset.theme = resolvedTheme.value;
  root.dataset.themePreference = mode.value;
  const meta = document.querySelector('meta[name="theme-color"]');
  meta?.setAttribute('content', THEME_COLORS[resolvedTheme.value]);
};

let mediaQuery: MediaQueryList | null = null;
const onSystemChange = (event: MediaQueryListEvent) => {
  systemDark.value = event.matches;
  applyTheme();
};

let initialized = false;

export const initializeTheme = () => {
  if (initialized) return;
  initialized = true;
  applyTheme();
  if (typeof window !== 'undefined' && typeof window.matchMedia === 'function') {
    mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
    systemDark.value = mediaQuery.matches;
    mediaQuery.addEventListener('change', onSystemChange);
    applyTheme();
  }
};

export const setTheme = (value: ThemeMode) => {
  if (!isThemeMode(value)) return;
  mode.value = value;
  try {
    window.localStorage.setItem(STORAGE_KEY, value);
  } catch {
  }
  applyTheme();
};

export const useTheme = () => ({
  themeMode: computed(() => mode.value),
  resolvedTheme,
  systemTheme,
  setTheme,
  pickTheme,
});
