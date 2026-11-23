import { defineMessages, messagesOf } from '../../i18n';


const messages = defineMessages(
  {
    vo2maxWhat: 'VO₂max: the oxygen your body can use per minute per kilogram of body weight (ml/kg/min).',
    vo2maxChart: 'It only updates after an outdoor run, a handful of times a year; a single reading says less than the long-term shape, which needs the 6-month range.',
    vo2maxHow: 'Estimated by the watch after outdoor runs, synced via advanced data — the official API has no VO₂max.',
    training_loadWhat: 'Training load: the rolling 7-day training load reported by Zepp, dimensionless.',
    training_loadChart: 'One point per day; the tier next to the latest reading (low / moderate / high / very high) follows a 600-point reference scale — a rough reading, not a watch-given grade.',
    training_loadHow: 'Given by the watch per day, synced via advanced data — the official API has no daily training load. The tier is drawn here against the reference scale.',
    pai_totalWhat: 'PAI: a Personal Activity Intelligence score over a rolling 7 days.',
    pai_totalChart: 'One point per day — the rolling 7-day total as of that day; the points newly earned that day are not what is drawn here.',
    pai_totalHow: 'Both the official authorization and advanced data provide PAI; Zepp keeps the rolling tally.',
    lactate_thresholdWhat: 'Lactate threshold: the point past which fatigue piles up quickly, marked by a heart rate and a pace.',
    lactate_thresholdChart: 'Two lines, heart rate and pace (the pace axis is inverted so faster points up); it only updates after a sustained hard run.',
    lactate_thresholdHow: 'Estimated by the watch after hard runs, synced via advanced data — the official API has no lactate threshold.',
    training_balanceWhat: 'Training load balance: the last 7 days of load against the 28-day weekly average — the acute-to-chronic ratio.',
    training_balanceChart: 'Three lines: 7-day load, 28-day weekly average, and the ratio; where a line breaks, its window was incomplete — uncomputed, not zero.',
    training_balanceHow: 'Computed here from individual workout loads, summed by local start date (not Zepp\'s rolling 7-day load, which would count each workout again); acute:chronic = 7-day sum ÷ (28-day sum ÷ 4). A day is complete only when its workout list was fully synced via advanced data and every workout has a valid load; only confirmed days without workouts count as zero, and incomplete 7- or 28-day windows stay empty. Official API workouts carry no load, so days synced only through the official API are left out.',
  },
);

export const TRAINING_METRICS = [
  'vo2max',
  'training_load',
  'pai_total',
  'lactate_threshold',
  'training_balance',
] as const;

export const trainingInfo = (metric: string): { what: string; chart: string; how: string } | null => {
  if (!(TRAINING_METRICS as readonly string[]).includes(metric)) return null;
  const t = messagesOf(messages) as Record<string, string>;
  return {
    what: t[`${metric}What`],
    chart: t[`${metric}Chart`],
    how: t[`${metric}How`],
  };
};
