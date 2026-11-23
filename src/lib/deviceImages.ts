import type { DeviceKind } from './deviceCatalog';

/**
 * Assets are discovered at build time. Adding a catalog row only requires an
 * image pair in this directory; there is no 48-item hand-maintained import
 * list to drift out of sync.
 */
const imageModules = import.meta.glob(['../assets/devices/*.webp', '!../assets/devices/*-thumb.webp'], {
  eager: true,
  import: 'default',
  query: '?url',
}) as Record<string, string>;
const thumbnailModules = import.meta.glob('../assets/devices/*-thumb.webp', {
  eager: true,
  import: 'default',
  query: '?url',
}) as Record<string, string>;

const keyFromPath = (path: string, suffix: string): string | null => {
  const normalizedPath = path.replace(/\\/g, '/');
  const match = normalizedPath.match(new RegExp(`/([^/]+)${suffix}$`));
  return match?.[1] ?? null;
};

const runtimeAssetUrl = (source: string): string => {
  if (/^(?:data:|https?:|asset:|blob:)/u.test(source)) return source;
  try {
    return new URL(source, import.meta.url).href;
  } catch {
    return source;
  }
};

export const localDeviceAssets: Readonly<Record<string, string>> = Object.freeze(
  Object.fromEntries(
    Object.entries(imageModules)
      .map(([path, source]) => [keyFromPath(path, '\\.webp'), runtimeAssetUrl(source)] as const)
      .filter((entry): entry is readonly [string, string] => Boolean(entry[0])),
  ),
);

export const localDeviceThumbnails: Readonly<Record<string, string>> = Object.freeze(
  Object.fromEntries(
    Object.entries(thumbnailModules)
      .map(([path, source]) => [keyFromPath(path, '-thumb\\.webp'), runtimeAssetUrl(source)] as const)
      .filter((entry): entry is readonly [string, string] => Boolean(entry[0])),
  ),
);

export function deviceImageFor(kind: DeviceKind | string | undefined, imageKey?: string | null): string {
  if (imageKey && localDeviceAssets[imageKey]) return localDeviceAssets[imageKey];
  return deviceFallbackFor(kind);
}

/** Missing images are rendered by DeviceVisual's inline, CSP-safe SVG fallback. */
export function deviceFallbackFor(_kind: DeviceKind | string | undefined): string {
  return '';
}

export function deviceThumbnailFor(kind: DeviceKind | string | undefined, imageKey?: string | null): string {
  if (imageKey && localDeviceThumbnails[imageKey]) return localDeviceThumbnails[imageKey];
  return deviceImageFor(kind, imageKey);
}
