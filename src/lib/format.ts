import { today as currentToday } from './currentDay';
import { displayDateTimeFormatter, parseDisplayDate } from './dateTime';
import { defineMessages, intlLocale, messagesOf } from '../i18n';
import type { FacePart } from './cards/faceValue';
import {
  bigDistanceThresholdMeters,
  distanceUnitLabel,
  shortDistanceUnitLabel,
  toBigDistance,
  toShortDistance,
} from './units';

export type HealthCategory = 'heart' | 'sleep' | 'activity';

const messages = defineMessages(
  {
    noUpdates: 'No updates yet',
    noRecords: 'No records yet',
    timeUnknown: 'Time unknown',
    dateUnknown: 'Date unknown',
    durationUnknown: 'Duration unknown',
    notRecorded: 'Not recorded',
    today: 'Today',
    yesterday: 'Yesterday',
    duration: (hours: number, minutes: number) =>
      (hours > 0 ? `${hours} hr ${minutes} min` : `${minutes} min`),
    hourUnit: 'h',
    minuteUnit: 'm',
    minutesUnit: 'min',
  },
);

const copy = () => messagesOf(messages);

export const isFiniteNumber = (value: unknown): value is number =>
  typeof value === 'number' && Number.isFinite(value);

export const localDateString = (date: Date): string => {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
};

export const formatWhen = (value?: string | null, now = currentToday()): string | null => {
  if (!value) return null;
  const date = parseDisplayDate(value);
  if (Number.isNaN(date.getTime())) return null;
  const time = displayDateTimeFormatter({ hour: '2-digit', minute: '2-digit' }).format(date);
  const dayStart = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((dayStart(now) - dayStart(date)) / 86_400_000);
  if (days === 0) return `${copy().today} ${time}`;
  if (days === 1) return `${copy().yesterday} ${time}`;
  const day = displayDateTimeFormatter({ month: '2-digit', day: '2-digit' }).format(date).replace(/\//g, '-');
  return `${day} ${time}`;
};

export const formatDateTime = (value?: string, empty = copy().noUpdates): string => {
  if (!value) return empty;
  const date = parseDisplayDate(value);
  if (Number.isNaN(date.getTime())) return empty;
  return displayDateTimeFormatter({
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date);
};

export const formatFullDateTime = (value?: string, empty = copy().noRecords): string => {
  if (!value) return empty;
  const date = parseDisplayDate(value);
  if (Number.isNaN(date.getTime())) return copy().timeUnknown;
  return displayDateTimeFormatter({
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date);
};

export type DateStyle = 'short' | 'long' | 'dated' | 'auto';

export const formatDate = (value: string, style: DateStyle = 'short', now: Date = new Date()): string => {
  const date = parseDisplayDate(value);
  if (Number.isNaN(date.getTime())) return copy().dateUnknown;
  if (style === 'long') {
    return displayDateTimeFormatter({
      year: 'numeric',
      month: 'long',
      day: 'numeric',
      weekday: 'long',
    }).format(date);
  }
  const withYear = style === 'dated' || (style === 'auto' && date.getFullYear() !== now.getFullYear());
  return displayDateTimeFormatter({
    ...(withYear ? { year: 'numeric' as const } : {}),
    month: 'short',
    day: 'numeric',
    weekday: 'short',
  }).format(date);
};

export const formatTime = (value: string): string => {
  const date = parseDisplayDate(value);
  return Number.isNaN(date.getTime())
    ? '—'
    : displayDateTimeFormatter({ hour: '2-digit', minute: '2-digit' }).format(date);
};

export const formatDuration = (minutes?: number | null, empty = copy().durationUnknown): string => {
  if (!isFiniteNumber(minutes) || minutes < 0) return empty;
  const total = Math.round(minutes);
  return copy().duration(Math.floor(total / 60), total % 60);
};

export const formatDurationParts = (minutes?: number | null): FacePart[] | null => {
  if (!isFiniteNumber(minutes) || minutes < 0) return null;
  const total = Math.round(minutes);
  const hours = Math.floor(total / 60);
  const rest = total % 60;
  const c = copy();
  return hours > 0
    ? [{ n: String(hours) }, { u: c.hourUnit }, { n: String(rest) }, { u: c.minuteUnit }]
    : [{ n: String(rest) }, { u: c.minutesUnit }];
};

export const formatDistance = (meters?: number, empty = copy().notRecorded): string => {
  if (!isFiniteNumber(meters) || meters <= 0) return empty;
  return meters >= bigDistanceThresholdMeters()
    ? `${toBigDistance(meters).toFixed(2)} ${distanceUnitLabel()}`
    : `${Math.round(toShortDistance(meters))} ${shortDistanceUnitLabel()}`;
};

export const formatNumber = (value: number, options?: Intl.NumberFormatOptions): string =>
  value.toLocaleString(intlLocale(), options).replace(/[  ]/g, ' ');

export const formatMetric = (value: number | undefined, digits = 0): string => {
  if (!isFiniteNumber(value)) return '—';
  return formatNumber(value, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });
};

export const formatBytes = (bytes: number | null | undefined, empty = '—'): string => {
  if (bytes === null || bytes === undefined || !Number.isFinite(bytes) || bytes <= 0) return empty;
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
};
