/**
 * The last gate for raw backend text.
 *
 * The UI shows text by code and falls back to the backend's raw text when it
 * does not know a code. The backend writes its raw text in Chinese. So the
 * fallback could show Chinese in the English UI.
 *
 * This gate never shows backend text that holds Chinese characters. It uses
 * the caller's generic text instead.
 */

/** CJK unified ideographs (U+4E00 to U+9FFF). This covers the Chinese the backend sends. */
const CJK = /[\u4e00-\u9fff]/;

/**
 * Decides if the backend's raw text can be shown.
 *
 * @param text the backend text, which can be Chinese
 * @param fallback the UI's generic text, used when `text` cannot be shown
 */
export const backendText = (
  text: string | null | undefined,
  fallback: string,
): string => {
  const value = (text ?? '').trim();
  if (!value) return fallback;
  return CJK.test(value) ? fallback : value;
};
