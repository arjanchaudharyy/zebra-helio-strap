import { describe, expect, it } from 'vitest';
import { failedChunkText } from '../failedChunkText';


const CHINESE = /[\u4e00-\u9fff]/;

const REAL_ROW = {
  error: '\u4e91\u7aef\u8fd4\u56de\u4e86\u62a5\u6587\uff0c\u4f46\u6ca1\u6709\u89e3\u6790\u51fa\u53ef\u7528\u8bb0\u5f55',
  error_code: 'err.backfill.no_canonical_records',
};

describe('failed chunk reason', () => {
  it('renders the row from the real database in English', () => {
    const text = failedChunkText(REAL_ROW);
    expect(text).not.toMatch(CHINESE);
    expect(text).toBe(
      'The cloud returned a payload, but no usable records were parsed',
    );
  });

  it('never shows Chinese for a legacy row that has no code', () => {
    const legacy = { error: '\u4e91\u7aef\u8fd4\u56de\u4e86\u62a5\u6587\uff0c\u4f46\u6ca1\u6709\u89e3\u6790\u51fa\u53ef\u7528\u8bb0\u5f55', error_code: null };
    const text = failedChunkText(legacy);
    expect(text).not.toMatch(CHINESE);
    expect(text).toBe('No reason recorded');
  });

  it('uses the shared error table for core error codes', () => {
    const networkFailure = { error: '\u65e0\u6cd5\u8fde\u63a5 Zepp \u533a\u57df\uff0c\u8bf7\u68c0\u67e5\u7f51\u7edc\u540e\u91cd\u8bd5', error_code: 'err.core.network' };
    const text = failedChunkText(networkFailure);
    expect(text).not.toMatch(CHINESE);
    expect(text).toContain('Zepp region');
  });

  it('handles a row with nothing recorded at all', () => {
    expect(failedChunkText({ error: null, error_code: null })).toBe('No reason recorded');
  });
});
