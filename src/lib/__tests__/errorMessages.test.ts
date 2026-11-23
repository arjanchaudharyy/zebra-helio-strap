import { describe, expect, it } from 'vitest';
import { errorTextFor } from '../../i18n/errors';
import { DesktopUnavailableError, toUserMessage } from '../bridge/errors';


const backendError = (code: string, message: string) => ({ code, message });

describe('backend errors in the English interface', () => {
  it('prefers the code over the backend prose', () => {
    const error = backendError('err.sync.not_connected', '\u5c1a\u672a\u8fde\u63a5 Zepp\uff0c\u8bf7\u5148\u5b8c\u6210\u8fde\u63a5');
    const shown = toUserMessage(error);
    expect(shown).toBe('Not connected to Zepp yet, connect first');
    expect(shown).not.toMatch(/[\u4e00-\u9fff]/);
  });

  it('localises the failure that used to strand people at sign-in', () => {
    const error = backendError(
      'err.login.credentials_unreadable',
      '\u5df2\u7ecf\u767b\u5f55\uff0c\u4f46\u6ca1\u80fd\u4ece\u767b\u5f55\u7a97\u53e3\u8bfb\u5230\u51ed\u636e\u3002\u53ef\u4ee5\u6539\u7528\u624b\u52a8\u586b\u5199 App Token\u3002',
    );
    expect(toUserMessage(error)).toContain('Enter an App Token manually');
  });

  it('falls back to the backend text when a code is unknown', () => {
    const error = backendError('err.brand.new_thing', 'something specific went wrong');
    expect(toUserMessage(error)).toBe('something specific went wrong');
  });

  it('still handles plain string errors from older code paths', () => {
    expect(toUserMessage('plain failure')).toBe('plain failure');
  });

  it('localises DesktopUnavailableError instead of dropping it at the CJK gate', () => {
    expect(toUserMessage(new DesktopUnavailableError())).toBe('Use the desktop app');
  });

  it('shows the English text for a known code', () => {
    const error = backendError('err.export.empty_range', '\u8fd9\u6bb5\u65f6\u95f4\u6ca1\u6709\u53ef\u5bfc\u51fa\u7684\u8bb0\u5f55');
    expect(toUserMessage(error)).toBe('No records in this range to export');
  });

  it('resolves every code through errorTextFor', () => {
    expect(errorTextFor('err.core.network')).toBe(
      'Could not reach the Zepp region, check your network and retry',
    );
    expect(errorTextFor(undefined)).toBeUndefined();
    expect(errorTextFor('err.not.a.real.code')).toBeUndefined();
  });
});
