import { describe, expect, it } from 'vitest';
import { backendText } from '../../i18n/backendText';
import { toUserMessage } from '../bridge/errors';


const CHINESE = /[\u4e00-\u9fff]/;

describe('backend prose never leaks into the English interface', () => {
  it('replaces Chinese with the caller fallback', () => {
    expect(backendText('\u4e91\u7aef\u8fd4\u56de\u4e86\u62a5\u6587\uff0c\u4f46\u6ca1\u6709\u89e3\u6790\u51fa\u53ef\u7528\u8bb0\u5f55', 'No reason recorded'))
      .toBe('No reason recorded');
  });

  it('lets non-Chinese backend text through', () => {
    expect(backendText('HTTP 503', 'fallback')).toBe('HTTP 503');
  });

  it('uses the fallback for empty or missing text', () => {
    expect(backendText(null, 'fallback')).toBe('fallback');
    expect(backendText('   ', 'fallback')).toBe('fallback');
  });

  it('protects toUserMessage even when the error carries no code at all', () => {
    const legacy = { message: '\u4e91\u7aef\u8fd4\u56de\u4e86\u62a5\u6587\uff0c\u4f46\u6ca1\u6709\u89e3\u6790\u51fa\u53ef\u7528\u8bb0\u5f55' };
    const shown = toUserMessage(legacy, 'Something went wrong');
    expect(shown).toBe('Something went wrong');
    expect(shown).not.toMatch(CHINESE);
  });

  it('still prefers a real code when there is one', () => {
    const shown = toUserMessage({ code: 'err.sync.not_connected', message: '\u5c1a\u672a\u8fde\u63a5 Zepp' });
    expect(shown).toBe('Not connected to Zepp yet, connect first');
  });
});
