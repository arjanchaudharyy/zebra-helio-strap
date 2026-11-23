import { computed, type ComputedRef } from 'vue';

/**
 * The UI text layer. The app is English only.
 *
 * Each module defines its own text with `defineMessages` and reads it with
 * `useMessages` (in components) or `messagesOf` (in plain functions).
 * Dates and numbers use `intlLocale()`; plurals use `plural()`.
 */
export type Locale = 'en';

/** The tag for `<html lang>`. */
const HTML_LANG = 'en';

/** The tag for `Intl`. Dates, weekdays and digit grouping use it. */
const INTL_LOCALE = 'en-US';

/** The current language. It is always English. */
export const locale = computed<Locale>(() => 'en');

// One text item: a sentence, a fixed list of short phrases, or a sentence with parameters.
type MessageLeaf = string | readonly string[] | ((...args: never[]) => string);

/** A module's text. It can nest, and a leaf can take parameters (a function). */
export type MessageTree = { readonly [key: string]: MessageLeaf | MessageTree };

export interface MessageBundle<T extends MessageTree> {
  readonly en: T;
}

/** Writes the language into `<html lang>` before the first render. */
export const initializeLocale = (): Promise<void> => {
  if (typeof document !== 'undefined') document.documentElement.lang = HTML_LANG;
  return Promise.resolve();
};

/** The tag to give `Intl.*`. Use it for all date and number formatting. */
export const intlLocale = (): string => INTL_LOCALE;

let pluralRules: Intl.PluralRules | null = null;

/** The plural forms. `other` is required: it is the fallback for every form. */
export interface PluralForms {
  readonly zero?: string;
  readonly one?: string;
  readonly two?: string;
  readonly few?: string;
  readonly many?: string;
  readonly other: string;
}

/**
 * A plural helper for function leaves:
 * `failedAttempts: (n) => plural(n, { one: `${n} attempt`, other: `${n} attempts` })`.
 */
export const plural = (count: number, forms: PluralForms): string => {
  pluralRules ??= new Intl.PluralRules(INTL_LOCALE);
  return forms[pluralRules.select(count)] ?? forms.other;
};

/* Zebra style: no em dashes (U+2014) in any English text. A spaced em dash becomes ", ", a bare one becomes "-".
   Function leaves are wrapped so formatted strings are cleaned too. Cached per bundle. */
const dashless = (text: string) => text.replace(/\s+\u2014\s+/g, ', ').replace(/\u2014/g, '-');
const cleanTree = (node: unknown): unknown => {
  if (typeof node === 'string') return dashless(node);
  if (typeof node === 'function') {
    const fn = node as (...args: unknown[]) => unknown;
    return (...args: unknown[]) => { const out = fn(...args); return typeof out === 'string' ? dashless(out) : out; };
  }
  if (node && typeof node === 'object' && !Array.isArray(node)) {
    return Object.fromEntries(Object.entries(node as Record<string, unknown>).map(([k, v]) => [k, cleanTree(v)]));
  }
  if (Array.isArray(node)) return node.map(cleanTree);
  return node;
};
const englishCache = new WeakMap<object, unknown>();
const plainEnglish = <T,>(tree: T): T => {
  const key = tree as unknown as object;
  const hit = englishCache.get(key);
  if (hit) return hit as T;
  const cleaned = cleanTree(tree) as T;
  englishCache.set(key, cleaned);
  return cleaned;
};

/** Defines the text of one module. */
export const defineMessages = <T extends MessageTree>(en: T): MessageBundle<T> => ({ en });

/** Reads a module's text in a component or composable. */
export const useMessages = <T extends MessageTree>(bundle: MessageBundle<T>): ComputedRef<T> =>
  computed(() => plainEnglish(bundle.en));

/** Reads a module's text in a plain function (for example in `format.ts`). */
export const messagesOf = <T extends MessageTree>(bundle: MessageBundle<T>): T =>
  plainEnglish(bundle.en);
