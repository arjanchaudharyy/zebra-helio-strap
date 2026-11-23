import { defineMessages, messagesOf } from '../../i18n';


const messages = defineMessages(
  {
    sleep_durationWhat: 'Time asleep: the night’s total time asleep, excluding stretches awake in bed.',
    sleep_durationChart: 'The big number is this night’s total; when you fell asleep, woke and the time in bed are on the line below.',
    sleep_durationHow: 'From the cloud’s sleep record — both the official authorization and advanced data provide it; when the cloud sends no time-asleep figure, it is worked out from the start, end and stage durations.',
    sleep_scoreWhat: 'Sleep score: the device’s own overall mark for the night, out of 100.',
    sleep_scoreChart: 'Shown as recorded — no reading into it here, and no diagnosis.',
    sleep_scoreHow: 'Given by the device with the night’s sleep record (both the official authorization and advanced data provide it) — not computed by ZeppBridge.',
    sleep_weeklyWhat: 'Sleep structure, last 7 nights: how long each stage took each night.',
    sleep_weeklyChart: 'One bar per night, stacked into deep / light / REM / awake; this record’s night is highlighted; a stage the cloud left out is labelled “Not provided”.',
    sleep_weeklyHow: 'Stage durations come with each sleep record — both the official authorization and advanced data provide them.',
    sleep_stagesWhat: 'Deep: the restorative stretch. Light: the transitional stage that takes up most of the night. REM: rapid eye movement, tied to memory and dreaming. Awake: waking up or lying awake in the night.',
    sleep_stagesChart: 'One timeline from falling asleep to waking, each stretch coloured by its stage; hover to see when it started, ended and how long it lasted.',
    sleep_stagesHow: 'The device splits the night into stages and they sync with the sleep record. These are definitions, not a health diagnosis.',
  },
);

export const SLEEP_METRICS = ['sleep_duration', 'sleep_score', 'sleep_weekly', 'sleep_stages'] as const;

export const sleepInfo = (metric: string): { what: string; chart: string; how: string } | null => {
  if (!(SLEEP_METRICS as readonly string[]).includes(metric)) return null;
  const t = messagesOf(messages) as Record<string, string>;
  return {
    what: t[`${metric}What`],
    chart: t[`${metric}Chart`],
    how: t[`${metric}How`],
  };
};
