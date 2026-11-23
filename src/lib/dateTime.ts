import { computed, ref } from 'vue';
import { defineMessages, intlLocale, messagesOf } from '../i18n';

export type TimeFormat = 'regional' | '12h' | '24h';
export type DateOrder = 'regional' | 'ymd' | 'dmy' | 'mdy';
export const TIME_FORMATS: readonly TimeFormat[] = ['regional', '12h', '24h'];
export const DATE_ORDERS: readonly DateOrder[] = ['regional', 'ymd', 'dmy', 'mdy'];
function saved<T extends string>(key: string, choices: readonly T[]): T {
  try {
    const value = localStorage.getItem(key) as T;
    if (choices.includes(value)) return value;
  } catch { /* Storage may be unavailable. */ }
  return choices[0]!;
}
const time = ref(saved('zeppbridge-time-format', TIME_FORMATS));
const order = ref(saved('zeppbridge-date-order', DATE_ORDERS));
export const timeFormat = computed(() => time.value);
export const dateOrder = computed(() => order.value);
export function setTimeFormat(value: TimeFormat) {
  if (!TIME_FORMATS.includes(value)) return;
  time.value = value;
  try { localStorage.setItem('zeppbridge-time-format', value); } catch { /* Session only. */ }
}
export function setDateOrder(value: DateOrder) {
  if (!DATE_ORDERS.includes(value)) return;
  order.value = value;
  try { localStorage.setItem('zeppbridge-date-order', value); } catch { /* Session only. */ }
}
const systemRegion = ref<string | null>(null);
export function setSystemRegion(value: string | null | undefined) {
  const tag = value?.trim().split('.')[0]?.split('@')[0]?.replace(/_/g, '-');
  try {
    systemRegion.value = tag ? Intl.getCanonicalLocales(tag)[0] ?? null : null;
  } catch {
    systemRegion.value = null;
  }
}

type ExplicitOrder = Exclude<DateOrder, 'regional'>;
const orderCache = new Map<string, ExplicitOrder>();
export function regionDateOrder(locale: string): ExplicitOrder {
  const cached = orderCache.get(locale);
  if (cached) return cached;
  const sequence = new Intl.DateTimeFormat(locale, { year: 'numeric', month: '2-digit', day: '2-digit' })
    .formatToParts(new Date(2001, 10, 22))
    .filter((part) => part.type === 'year' || part.type === 'month' || part.type === 'day')
    .map((part) => part.type[0])
    .join('');
  const found: ExplicitOrder = sequence === 'dmy' ? 'dmy' : sequence === 'mdy' ? 'mdy' : 'ymd';
  orderCache.set(locale, found);
  return found;
}

const words = defineMessages(
  { time: 'Time format', date: 'Date format', regional: 'System region', '12h': '12-hour', '24h': '24-hour', ymd: 'Year/month/day', dmy: 'Day/month/year', mdy: 'Month/day/year' },
);
export const dateTimeLabels = computed(() => messagesOf(words));

/** Calendar dates are local dates; ISO timestamps retain their explicit offset. */
export function parseDisplayDate(value: string): Date {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return new Date(value);
  const [year, month, day] = match.slice(1).map(Number) as [number, number, number];
  const date = new Date(year, month - 1, day);
  return date.getFullYear() === year && date.getMonth() === month - 1 && date.getDate() === day
    ? date : new Date(NaN);
}

const formatterCache = new Map<string, Intl.DateTimeFormat>();

const optionsKey = (options: Intl.DateTimeFormatOptions): string =>
  JSON.stringify(
    Object.keys(options)
      .sort()
      .map((name) => [name, (options as Record<string, unknown>)[name]]),
  );

const hour12Cache = new Map<string, boolean | undefined>();
const probeSystemHour12 = (): boolean | undefined => {
  const region = systemRegion.value ?? '';
  if (!hour12Cache.has(region)) {
    hour12Cache.set(region, new Intl.DateTimeFormat(region || undefined, { hour: 'numeric' }).resolvedOptions().hour12);
  }
  return hour12Cache.get(region);
};

function blendLocale(language: string, systemLocale: string): string | undefined {
  try {
    const country = new Intl.Locale(systemLocale).region;
    if (!country) return undefined;
    return Intl.getCanonicalLocales(`${new Intl.Locale(language).language}-${country}`)[0];
  } catch {
    return undefined;
  }
}

/** Shared display policy. Never changes exported timestamps or source timezone semantics. */
export function displayDateTimeFormatter(options: Intl.DateTimeFormatOptions = {}): Intl.DateTimeFormat {
  const key = `${intlLocale()}|${systemRegion.value ?? ''}|${time.value}|${order.value}|${optionsKey(options)}`;
  const cached = formatterCache.get(key);
  if (cached) return cached;
  const adjusted = { ...options };
  delete adjusted.hour12;
  if (adjusted.hour || adjusted.timeStyle) {
    if (time.value !== 'regional') adjusted.hourCycle = time.value === '12h' ? 'h12' : 'h23';
    else if (systemRegion.value) adjusted.hour12 = probeSystemHour12();
  }
  let region: string | undefined = intlLocale();
  let effectiveOrder: DateOrder = order.value;
  if (effectiveOrder === 'regional' && systemRegion.value) {
    const wanted = regionDateOrder(systemRegion.value);
    if (wanted !== regionDateOrder(region)) {
      const blended = blendLocale(region, systemRegion.value);
      const hasWords = Boolean(adjusted.weekday || adjusted.dateStyle || adjusted.era)
        || ['short', 'long', 'narrow'].includes(String(adjusted.month));
      if (blended && regionDateOrder(blended) === wanted) region = blended;
      else if (!hasWords) effectiveOrder = wanted;
    }
  }
  if (effectiveOrder !== 'regional' && (adjusted.day || adjusted.dateStyle)) {
    region = { ymd: 'sv-SE', dmy: 'en-GB', mdy: 'en-US' }[effectiveOrder];
    if (!adjusted.dateStyle) {
      adjusted.month = '2-digit';
      adjusted.day = '2-digit';
    }
    // Date order must not choose the hour cycle on behalf of the user.
    if (time.value === 'regional' && (adjusted.hour || adjusted.timeStyle)) {
      adjusted.hour12 = probeSystemHour12();
    }
  }
  const formatter = new Intl.DateTimeFormat(region, adjusted);
  formatterCache.set(key, formatter);
  return formatter;
}
