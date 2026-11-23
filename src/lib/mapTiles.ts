
export interface TileProvider {
  url: string;
  origin: string;
  maxZoom: number;
  attribution: string;
  attributionUrl: string;
}

export const MAP_TILES: TileProvider = {
  url: 'https://tile.openstreetmap.org/{z}/{x}/{y}.png',
  origin: 'https://tile.openstreetmap.org',
  maxZoom: 19,
  attribution: '© OpenStreetMap',
  attributionUrl: 'https://www.openstreetmap.org/copyright',
};

export type RouteViewMode = 'local' | 'map';

const CONSENT_KEY = 'zeppbridge-route-map-consent';
const MODE_KEY = 'zeppbridge-route-map-mode';

export const mapAlwaysAllowed = (): boolean => {
  try {
    return window.localStorage.getItem(CONSENT_KEY) === 'always';
  } catch {
    return false;
  }
};

export const allowMapAlways = (): void => {
  try {
    window.localStorage.setItem(CONSENT_KEY, 'always');
  } catch {
  }
};

export const initialRouteMode = (): RouteViewMode => {
  if (!mapAlwaysAllowed()) return 'local';
  try {
    return window.localStorage.getItem(MODE_KEY) === 'map' ? 'map' : 'local';
  } catch {
    return 'local';
  }
};

export const rememberRouteMode = (mode: RouteViewMode): void => {
  if (!mapAlwaysAllowed()) return;
  try {
    window.localStorage.setItem(MODE_KEY, mode);
  } catch {
  }
};
