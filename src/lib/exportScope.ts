export const DEFAULT_EXPORT_FORMAT_KEY = 'zeppbridge-default-export-format';
export const DEFAULT_EXPORT_FORMATS = ['json', 'csv', 'gpx'] as const;
export type DefaultExportFormat = (typeof DEFAULT_EXPORT_FORMATS)[number];

export const isDefaultExportFormat = (value: unknown): value is DefaultExportFormat =>
  value === 'json' || value === 'csv' || value === 'gpx';

export const readDefaultExportFormat = (): DefaultExportFormat => {
  if (typeof window === 'undefined') return 'json';
  try {
    const raw = window.localStorage.getItem(DEFAULT_EXPORT_FORMAT_KEY);
    if (isDefaultExportFormat(raw)) return raw;
  } catch {
  }
  return 'json';
};

export const writeDefaultExportFormat = (format: DefaultExportFormat): void => {
  if (!isDefaultExportFormat(format)) return;
  try {
    window.localStorage.setItem(DEFAULT_EXPORT_FORMAT_KEY, format);
  } catch {
  }
};
