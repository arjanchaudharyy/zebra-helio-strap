import { defineMessages, messagesOf } from '../../i18n';


const messages = defineMessages(
  {
    heart_rate_24hWhat: 'Heart rate, reading by reading, over the last 24 hours, 48 hours, or 7 days.',
    heart_rate_24hChart: 'The big number is the latest reading; stretches without the watch on break the line rather than filling in 0. Switch between 24h, 48h, and 7d in the card header.',
    heart_rate_24hHow: 'Samples come from the official authorization or advanced data; the latest / average / lowest / highest figures in this card are computed here from the selected window.',
    daily_max_hrWhat: 'The day’s peak, average, and minimum heart rate, counted from the raw samples stored on this machine.',
    daily_max_hrChart: 'Three lines: the day’s peak, average, and min; days with very few samples (under 60) use hollow markers on peak and min — they are only the extremes of those few points.',
    daily_max_hrHow: 'Counted here from local samples without the filtering the Zepp app applies, so the two numbers differing is expected. Samples from both the official authorization and advanced data are included.',
  },
);

export const HEART_METRICS = ['heart_rate_24h', 'daily_max_hr'] as const;

export const heartInfo = (metric: string): { what: string; chart: string; how: string } | null => {
  if (!(HEART_METRICS as readonly string[]).includes(metric)) return null;
  const t = messagesOf(messages) as Record<string, string>;
  return {
    what: t[`${metric}What`],
    chart: t[`${metric}Chart`],
    how: t[`${metric}How`],
  };
};
