interface DemoFlags { demo: boolean; theme: string | null; lang: string | null; showcase: boolean; scene?: string; tour?: string; welcome?: boolean; ret?: string }

declare global {
  interface Window { __ZB_DEMO__?: DemoFlags }
}

const read = (): DemoFlags | undefined => {
  try {
    const params = new URLSearchParams(window.location.search);
    const value = params.get('demo');
    if (value === null || value === '0' || value === 'false') return undefined;
    const route = params.get('route');
    if (route && route.startsWith('/') && !route.startsWith('//')) window.history.replaceState(null, '', route);
    const tour = params.get('tour') ?? undefined;
    return {
      demo: true, theme: params.get('theme'), lang: params.get('lang'), showcase: params.get('showcase') === '1',
      scene: params.get('scene') ?? undefined, tour, welcome: params.get('welcome') === '1', ret: params.get('ret') ?? undefined,
    };
  } catch {
    return undefined;
  }
};

const applyRouteParam = () => {
  try {
    const params = new URLSearchParams(window.location.search);
    if (params.get('demo') !== null) return;
    const route = params.get('route');
    if (route && route.startsWith('/') && !route.startsWith('//')) window.history.replaceState(null, '', route);
  } catch {
  }
};

if (typeof window !== 'undefined' && window.__ZB_DEMO__ === undefined) {
  applyRouteParam();
  const flags = read();
  if (flags) {
    window.__ZB_DEMO__ = flags;
    // App preferences inside a demo must never overwrite the visitor's real preferences.
    // Every iframe gets its own disposable Storage, so resetting one demo affects nothing else.
    const values = new Map<string, string>();
    const memory: Storage = {
      get length() { return values.size; }, clear: () => values.clear(),
      getItem: key => values.get(key) ?? null, setItem: (key, value) => { values.set(key, String(value)); },
      removeItem: key => { values.delete(key); }, key: index => [...values.keys()][index] ?? null,
    };
    if (flags.tour !== undefined) {
      try { window.__ZB_REAL_STORAGE__ = window.localStorage; } catch { }
    }
    Object.defineProperty(window, 'localStorage', { configurable: true, value: memory });
    Object.defineProperty(window, 'sessionStorage', { configurable: true, value: memory });
    values.set('zeppbridge-theme', flags.theme ?? 'light');
    values.set('zeppbridge-locale', flags.lang ?? 'en');
    values.set('zb.focusAsked', '1');
  }
}

export {};
