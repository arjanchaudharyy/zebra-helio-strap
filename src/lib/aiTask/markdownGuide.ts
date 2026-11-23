import { defineMessages, messagesOf } from '../../i18n';

const messages = defineMessages(
  {
    guide: [
      'How to read this file:',
      '- Every table is a CSV code block; the first row is the header and units are in parentheses.',
      '- An empty cell means no data for that day or moment — it is not 0. Do not fill in or guess.',
      '- Daily metrics: one row per day. Sleep: one row per night. Workouts: one row per workout. Coverage: the date range and how many days actually have data, per category.',
      '- Workout curves (Samples) are averaged over curve_average_seconds (see Summary); heart_rate_min / heart_rate_max are the lowest and highest in each step. Whole-workout statistics come from the full per-second data, under Curve statistics.',
      '- Workouts listed in workouts_summary_only keep only their summary row so the file stays readable.',
    ].join('\n'),
  },
);

export const markdownGuide = (): string => messagesOf(messages).guide;
