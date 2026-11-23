import { defineMessages, messagesOf } from '../i18n';
import type { LifeEventInput } from '../types';
import type { ChartPalette } from './echartsTheme';
import { displayDateTimeFormatter, parseDisplayDate } from './dateTime';

export const eventCategories = ['health', 'travel', 'routine', 'training', 'other'] as const;
export const lifeEventMessages = defineMessages(
  {
    title: 'Life events', intro: 'Record what happened alongside your health data.',
    add: 'Add event', edit: 'Edit event', empty: 'No life events yet. Start with an illness, a trip, or a training change.',
    name: 'Title', category: 'Category', start: 'Start date', end: 'End date', ongoing: 'Still ongoing',
    notes: 'Notes (optional)', placeholder: 'e.g. a cold, a few days off training', save: 'Save', cancel: 'Cancel',
    remove: 'Delete', deleteTitle: 'Delete this life event?', deleteHint: 'Removed from the local database.',
    invalid: 'Enter a title and valid dates. End date cannot be before start date.', failed: 'Action failed. Try again.',
    loading: 'Loading life events…', retry: 'Retry',
    since: (date: string) => `from ${date}`, chartKey: 'Life events',
    today: 'Today', yesterday: 'Yesterday', sameAsStart: 'Same as start',
    active: 'Ongoing', search: 'Search life events', noMatch: 'No matching events.',
    all: 'All', showMore: (count: number) => `Show ${count} more`, showLess: 'Show less', manage: 'Manage life events', related: 'Related events',
    local: 'Stored on this computer and kept in backups. Eras go to the AI automatically; a connected local AI can read and add events too, and every change can be undone.',
    epoch: 'Mark as an era', epochHint: 'Name this stretch of time: every trend chart gets a shaded backdrop, and AI sees it too.', epochBadge: 'Era',
    categories: { health: 'Health & recovery', travel: 'Travel', routine: 'Routine & lifestyle', training: 'Training & races', other: 'Other' },
    categoryShort: { health: 'Health', travel: 'Travel', routine: 'Routine', training: 'Training', other: 'Other' },
  },
);

export function validEventDate(date: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) return false;
  const parsed = new Date(`${date}T00:00:00Z`);
  return Number.isFinite(parsed.getTime()) && parsed.toISOString().slice(0, 10) === date;
}
export function validLifeEvent(event: LifeEventInput): boolean {
  return Boolean(event.title.trim()) && Array.from(event.title.trim()).length <= 120
    && Array.from(event.notes).length <= 4000 && validEventDate(event.startDate)
    && (event.endDate === null || (validEventDate(event.endDate) && event.endDate >= event.startDate));
}
export const eventTone = (category: string): string => ({
  health: 'var(--heart)',
  travel: 'var(--pace)',
  routine: 'var(--sleep)',
  training: 'var(--training)',
}[category] ?? 'var(--subtle)');

export const eventIcon = (category: string): 'heart' | 'map' | 'moon' | 'run' | 'star' => ({
  health: 'heart' as const,
  travel: 'map' as const,
  routine: 'moon' as const,
  training: 'run' as const,
}[category] ?? 'star');

export const overlapsEvent =(event: LifeEventInput, start: string, end: string): boolean =>
  event.startDate <= end && (event.endDate === null || event.endDate >= start);

export const eventChartTone = (category: string, palette: ChartPalette): string => ({
  health: palette.series.heart,
  travel: palette.series.pace,
  routine: palette.series.sleep.light,
  training: palette.series.training,
}[category] ?? palette.legendOff);

export const eventSpanLabel = (event: LifeEventInput): string => {
  const format = displayDateTimeFormatter({ month: 'numeric', day: 'numeric' });
  const day = (date: string) => format.format(parseDisplayDate(date));
  if (event.endDate === null) return messagesOf(lifeEventMessages).since(day(event.startDate));
  if (event.endDate === event.startDate) return day(event.startDate);
  return `${day(event.startDate)}–${day(event.endDate)}`;
};
