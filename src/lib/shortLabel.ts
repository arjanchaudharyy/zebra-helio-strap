/**
 * The short name of a capsule (the "short name" step of the SegmentTrack fallback ladder).
 */
import { messagesOf, type MessageBundle, type MessageTree } from '../i18n';

export const ownShort = <T extends MessageTree>(
  bundle: MessageBundle<T>,
  pick: (tree: T) => string | undefined,
): string | undefined => pick(messagesOf(bundle));
