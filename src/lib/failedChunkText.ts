import { defineMessages, messagesOf } from '../i18n';
import { backendText } from '../i18n/backendText';
import { errorTextFor } from '../i18n/errors';

const messages = defineMessages(
  {
    noCanonical: 'The cloud returned a payload, but no usable records were parsed',
    noReason: 'No reason recorded',
  },
);

export interface FailedChunkLike {
  readonly error?: string | null;
  readonly error_code?: string | null;
}

export const failedChunkText = (chunk: FailedChunkLike): string => {
  const t = messagesOf(messages);
  if (chunk.error_code === 'err.backfill.no_canonical_records') return t.noCanonical;
  const localized = errorTextFor(chunk.error_code);
  if (localized) return localized;
  return backendText(chunk.error, t.noReason);
};
