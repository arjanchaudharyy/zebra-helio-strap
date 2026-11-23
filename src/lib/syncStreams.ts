import { defineMessages, messagesOf } from '../i18n';

const messages = defineMessages(
  {
    heart_rate: 'Heart rate',
    daily_summary: 'Daily summaries',
    sleep: 'Sleep',
    hrv: 'Heart rate variability',
    wellness: 'Stress, SpO2 and other optional metrics',
    workouts: 'Workouts',
    workout_detail: 'Workout detail and tracks',
    weight: 'Weight and body composition',

    vo2max: 'VO₂max',
    lactate_threshold_hr: 'Lactate threshold heart rate',
    lactate_threshold_pace: 'Lactate threshold pace',
    resting_heart_rate: 'Resting heart rate',
    training_load: 'Training load',
    blood_oxygen: 'Blood oxygen',
    breathing_rate: 'Respiratory rate',
    skin_temperature: 'Skin temperature',
  },
);

export const syncStreamLabel = (stream: string, fallback?: string): string =>
  (messagesOf(messages) as Record<string, string | undefined>)[stream]
  ?? fallback
  ?? stream;
