import { isTourChapter, type TourChapterId } from './chapters';
import { TOUR_VERSION, needsRetour } from './progress';

declare global {
  interface Window { __ZB_REAL_STORAGE__?: Storage }
}

const SEEN_KEY = 'zeppbridge.tour.seen';
const FIRST_LAUNCH_KEY = 'zeppbridge.tour.firstLaunch';
const VERSION_KEY = 'zeppbridge.tour.version';
const TOOLS_REMINDER_KEY = 'zeppbridge.tour.toolsReminder';
const THEME_KEY = 'zeppbridge-theme';
const LOCALE_KEY = 'zeppbridge-locale';

const realStorage = (): Storage | null => {
  try {
    return window.__ZB_REAL_STORAGE__ ?? window.localStorage;
  } catch {
    return null;
  }
};

export const seenChapters = (): Set<string> => {
  const seen = new Set<string>();
  try {
    const storage = realStorage();
    const parsed = JSON.parse(storage?.getItem(SEEN_KEY) ?? '[]') as unknown;
    if (Array.isArray(parsed)) for (const item of parsed) if (typeof item === 'string' && isTourChapter(item)) seen.add(item);
  } catch {
  }
  return seen;
};

export const markSeen = (ids: readonly string[]): void => {
  const seen = seenChapters();
  for (const id of ids) seen.add(id);
  try {
    realStorage()?.setItem(SEEN_KEY, JSON.stringify([...seen]));
  } catch {
  }
};

export const forgetSeen = (ids: readonly string[]): void => {
  const seen = seenChapters();
  for (const id of ids) seen.delete(id);
  try {
    realStorage()?.setItem(SEEN_KEY, JSON.stringify([...seen]));
  } catch {
  }
};

export const firstLaunchHandled = (): boolean => {
  try {
    return realStorage()?.getItem(FIRST_LAUNCH_KEY) === '1';
  } catch {
    return true;
  }
};

export const markFirstLaunchHandled = (): void => {
  try {
    realStorage()?.setItem(FIRST_LAUNCH_KEY, '1');
  } catch {
  }
};

export const retourDue = (): boolean => {
  try {
    const storage = realStorage();
    return needsRetour(storage?.getItem(FIRST_LAUNCH_KEY) === '1', storage?.getItem(VERSION_KEY) ?? null);
  } catch {
    return false;
  }
};

export const introPending = (): boolean => !firstLaunchHandled() || retourDue();

export const markTourVersion = (fresh = false): void => {
  try {
    const storage = realStorage();
    if (fresh) {
      storage?.removeItem(SEEN_KEY);
    }
    storage?.setItem(VERSION_KEY, String(TOUR_VERSION));
  } catch {
  }
};

export const markToolsReminder = (): void => {
  try {
    realStorage()?.setItem(TOOLS_REMINDER_KEY, '1');
  } catch {
  }
};

export const takeToolsReminder = (): boolean => {
  try {
    const storage = realStorage();
    if (storage?.getItem(TOOLS_REMINDER_KEY) !== '1') return false;
    storage.removeItem(TOOLS_REMINDER_KEY);
    return true;
  } catch {
    return false;
  }
};

export interface SandboxFlags {
  chapters: TourChapterId[];
  welcome: boolean;
  ret: string;
}

const safePath = (value: string | null | undefined, fallback: string): string =>
  value && value.startsWith('/') && !value.startsWith('//') ? value : fallback;

export const sandboxFlags = (): SandboxFlags | null => {
  const flags = window.__ZB_DEMO__;
  if (!flags?.demo || !flags.tour) return null;
  const chapters = flags.tour.split(',').filter(isTourChapter);
  if (!chapters.length && !flags.welcome) return null;
  return { chapters, welcome: flags.welcome === true, ret: safePath(flags.ret, '/') };
};

export const inSandbox = (): boolean => sandboxFlags() !== null;

export const reloadUrl = (path: string | null | undefined): string => {
  const target = safePath(path, '/');
  return target === '/' ? '/' : `/?${new URLSearchParams({ route: target }).toString()}`;
};

export const enterSandbox = (options: { chapters: TourChapterId[]; welcome?: boolean; ret: string; route: string; theme: string; locale: string }): void => {
  const params = new URLSearchParams({
    demo: '1',
    tour: options.chapters.join(','),
    ret: safePath(options.ret, '/'),
    route: safePath(options.route, '/'),
    theme: options.theme,
    lang: options.locale,
  });
  if (options.welcome) params.set('welcome', '1');
  window.location.assign(`/?${params.toString()}`);
};

export const leaveSandbox = (ret: string): void => {
  const real = window.__ZB_REAL_STORAGE__;
  const flags = window.__ZB_DEMO__;
  try {
    const memory = window.localStorage;
    const locale = memory.getItem(LOCALE_KEY);
    const theme = memory.getItem(THEME_KEY);
    if (real && locale && locale !== flags?.lang) real.setItem(LOCALE_KEY, locale);
    if (real && theme && theme !== flags?.theme) real.setItem(THEME_KEY, theme);
  } catch {
  }
  window.location.replace(reloadUrl(ret));
};
