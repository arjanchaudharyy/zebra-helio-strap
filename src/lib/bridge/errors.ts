import { defineMessages, messagesOf } from '../../i18n';
import { errorTextFor } from '../../i18n/errors';
import { backendText } from '../../i18n/backendText';

type UnknownRecord = Record<string, unknown>;

const messages = defineMessages(
  {
    desktopOnly: 'Use the desktop app',
    genericFailure: 'Action failed. Try again shortly',
    timedOut: 'Request timed out. Check your network and the Zepp region, then try again.',
  },
);

const copy = () => messagesOf(messages);

// The backend says "use the desktop app" in Chinese.
const DESKTOP_ONLY_MARKER = '\u8bf7\u4f7f\u7528\u684c\u9762\u5e94\u7528';

export class DesktopUnavailableError extends Error {
  constructor(message = DESKTOP_ONLY_MARKER) {
    super(message);
    this.name = 'DesktopUnavailableError';
  }
}

export class TauriUnavailableError extends DesktopUnavailableError {
  constructor(message = DESKTOP_ONLY_MARKER) {
    super(message);
    this.name = 'TauriUnavailableError';
  }
}

const errorText = (error: unknown): string => {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null) {
    const candidate = error as UnknownRecord;
    if (typeof candidate.message === 'string') return candidate.message;
    if (typeof candidate.error === 'string') return candidate.error;
  }
  return '';
};

export const errorCode = (error: unknown): string | undefined => {
  if (typeof error === 'object' && error !== null) {
    const candidate = error as UnknownRecord;
    if (typeof candidate.code === 'string' && candidate.code) return candidate.code;
  }
  return undefined;
};

export const toUserMessage = (error: unknown, fallback = copy().genericFailure): string => {
  const localized = errorTextFor(errorCode(error));
  if (localized) return localized;

  if (error instanceof DesktopUnavailableError) {
    return copy().desktopOnly;
  }

  const source = errorText(error).replace(/^Err\((.*)\)$/s, '$1').trim();
  if (!source) return fallback;
  if (source.includes(DESKTOP_ONLY_MARKER)) {
    return copy().desktopOnly;
  }
  if (backendText(source, '') === '') return fallback;
  const lower = source.toLowerCase();
  if (lower.includes('timed out') || lower.includes('timeout')) {
    return copy().timedOut;
  }
  if (source.length > 140) return `${source.slice(0, 137)}…`;
  return source;
};
