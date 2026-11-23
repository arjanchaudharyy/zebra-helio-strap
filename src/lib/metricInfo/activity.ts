import { defineMessages, messagesOf } from '../../i18n';


const messages = defineMessages(
  {
    stepsWhat: 'Steps: the day’s step total as the watch sums it up.',
    stepsChart: 'One point per day; days without a record break the line rather than filling in 0. The hour-by-hour shape is in the hourly card above.',
    stepsHow: 'Both the official authorization and advanced data provide daily steps; the hourly card leans on the legacy channel’s per-minute records first and fills gaps with the official hourly summary.',
    distanceWhat: 'Distance: how far you moved that day in total.',
    distanceChart: 'One point per day; days without a record break the line rather than filling in 0.',
    distanceHow: 'Both the official authorization and advanced data provide it, synced with the daily activity summary.',
    active_caloriesWhat: 'Active burn: the day’s activity-only burn, basal metabolism excluded.',
    active_caloriesChart: 'One point per day; days without a record break the line rather than filling in 0.',
    active_caloriesHow: 'Both the official authorization and advanced data provide it, kept separate from the day’s total burn.',
    active_minutesWhat: 'Active minutes: the minutes the watch counted as activity.',
    active_minutesChart: 'One point per day; days without a record break the line rather than filling in 0.',
    active_minutesHow: 'Via advanced data — the official daily summary does not carry active minutes yet.',
  },
);

export const ACTIVITY_METRICS = [
  'steps',
  'distance',
  'active_calories',
  'active_minutes',
] as const;

export const activityInfo = (metric: string): { what: string; chart: string; how: string } | null => {
  if (!(ACTIVITY_METRICS as readonly string[]).includes(metric)) return null;
  const t = messagesOf(messages) as Record<string, string>;
  return {
    what: t[`${metric}What`],
    chart: t[`${metric}Chart`],
    how: t[`${metric}How`],
  };
};
