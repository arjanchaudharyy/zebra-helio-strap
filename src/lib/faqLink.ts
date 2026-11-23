/**
 * Links to the English FAQ and guides.
 * The anchors are hand-written `<a id>` tags in the docs.
 */
import { backend } from './bridge';

const BASE = 'https://github.com/lingcang728/ZeppBridge/blob/main/docs/guides';

export type FaqAnchor = 'relay' | 'missing-data' | 'helio' | 'windows-warning';
export type Guide = 'ai';

export const faqUrl = (anchor: FaqAnchor): string =>
  `${BASE}/faq.md#${anchor}`;

export const guideUrl = (guide: Guide): string =>
  `${BASE}/${guide}.md`;

const openQuietly = (url: string): void => {
  void backend.openExternalUrl(url).catch(() => undefined);
};

export const openFaq = (anchor: FaqAnchor): void => openQuietly(faqUrl(anchor));
export const openGuide = (guide: Guide): void => openQuietly(guideUrl(guide));
