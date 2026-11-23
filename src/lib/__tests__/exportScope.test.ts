import { describe, expect, it } from 'vitest';
import { isDefaultExportFormat, readDefaultExportFormat } from '../exportScope';

describe('default export format', () => {
  it('accepts only json / csv / gpx', () => {
    expect(isDefaultExportFormat('json')).toBe(true);
    expect(isDefaultExportFormat('fit')).toBe(false);
    expect(isDefaultExportFormat('xml')).toBe(false);
  });

  it('falls back to json when there is no browser storage', () => {
    expect(readDefaultExportFormat()).toBe('json');
  });
});
