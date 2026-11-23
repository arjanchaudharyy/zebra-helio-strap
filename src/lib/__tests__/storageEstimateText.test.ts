import { describe, expect, it } from 'vitest';
import { storageEstimateText, storageStopReasonText } from '../storageEstimateText';


const base = {
  message: '\u6309\u672c\u673a\u5df2\u6709\u6570\u636e\u7684\u5b9e\u9645\u901f\u7387\u63a8\u7b97\uff0c365 \u5929\u5927\u7ea6\u5360\u7528 514 MB\uff0c\u672c\u76d8\u5269\u4f59 163.1 GB\u3002',
  requested_days: 365,
  estimated_add_bytes: 514 * 1024 * 1024,
  free_bytes: 175_000_000_000,
  needed_bytes: 600 * 1024 * 1024,
};

const CHINESE = /[\u4e00-\u9fff]/;

describe('storage estimate copy', () => {
  it('never shows Chinese in English for any known code', () => {
    const codes = [
      'ui.estimate.stop_no_space',
      'ui.estimate.disk_unknown',
      'ui.estimate.disk_too_small',
      'ui.estimate.builtin_guess',
      'ui.estimate.measured',
      'ui.estimate.partial',
    ];
    for (const message_code of codes) {
      const text = storageEstimateText({ ...base, message_code });
      expect(text, message_code).not.toMatch(CHINESE);
      expect(text.length, message_code).toBeGreaterThan(0);
    }
  });

  it('renders the sentence from the screenshot in English', () => {
    const text = storageEstimateText({ ...base, message_code: 'ui.estimate.measured' });
    expect(text).toContain('365 days');
    expect(text).toContain('the rate your own data accumulates');
    expect(text).not.toMatch(CHINESE);
  });

  it('does not fall back to Chinese for an unknown code', () => {
    const text = storageEstimateText({ ...base, message_code: 'ui.estimate.brand_new' });
    expect(text).not.toMatch(CHINESE);
    expect(text).toBe('Cannot estimate this backfill’s size right now.');
  });

  it('renders the stop reason without Chinese', () => {
    const text = storageStopReasonText({
      ...base,
      stop_reason: '\u8fd9\u6b21\u8865\u62c9\u9884\u8ba1\u9700\u8981 600 MB\uff08\u542b\u5b89\u5168\u4f59\u91cf\uff09\uff0c\u672c\u76d8\u53ea\u5269 100 MB\uff0c\u4e0d\u4f1a\u5f00\u59cb\u3002',
      stop_reason_code: 'ui.estimate.stop_no_space',
    });
    expect(text).not.toMatch(CHINESE);
    expect(text).toContain('safety margin');
  });

  it('returns empty when there is nothing to say', () => {
    expect(storageEstimateText(null)).toBe('');
    expect(storageStopReasonText({ ...base, stop_reason: null })).toBe('');
  });
});
