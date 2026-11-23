import catalogJson from '../assets/devices/catalog.json';

export type DeviceKind = 'watch' | 'strap' | 'ring' | 'band' | 'earbuds' | 'scale' | 'unknown';
export type DeviceCatalogStatus = 'active' | 'historical';

export interface DeviceCatalogEntry {
  catalog_id: string;
  canonical_name: string;
  display_name: string;
  name_zh?: string | null;
  kind: DeviceKind;
  model_codes: string[];
  aliases: string[];
  region: string[];
  status: DeviceCatalogStatus;
  supported: boolean;
  canonical_device_key?: string;
  official_page?: string;
  official_url: string;
  image_source_url?: string | null;
  asset_source?: string | null;
  image_key?: string | null;
  asset_hash?: string | null;
  checked_at: string;
  provenance: string;
}

export interface DeviceCatalogDocument {
  version: number;
  checked_at: string;
  sources: string[];
  devices: DeviceCatalogEntry[];
}

const document = catalogJson as DeviceCatalogDocument;

/** Versioned snapshot of the official catalog. No runtime network lookup is performed. */
export const deviceCatalog: readonly DeviceCatalogEntry[] = document.devices;

export { deviceFallbackFor, deviceImageFor, deviceThumbnailFor, localDeviceAssets, localDeviceThumbnails } from './deviceImages';

export function catalogEntryMatchesId(
  entry: DeviceCatalogEntry,
  value?: string | null,
): boolean {
  if (!value) return false;
  return entry.catalog_id === value
    || entry.canonical_device_key === value
    || entry.canonical_name === value;
}
