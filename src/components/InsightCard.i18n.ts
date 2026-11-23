import { defineMessages } from '../i18n';

export const insightCardMessages = defineMessages(
  {
    title: 'How the run went',
    unsupportedWorkoutType: 'No insights for this workout type yet. The first version covers running only — what has been verified against real data. Other workouts still display, correct and export normally.',
    currentRun: 'This run',
    baselineRun: 'Baseline',
    reading: 'Reading local records…',
    comparedTo: (count: number) => `Against your own ${count} most recent runs of a similar distance:`,
    noComparison: 'Not enough comparable history yet — numbers only, no comparison.',
    driftTitle: 'First half vs second',
    driftFirst: 'First half',
    driftSecond: 'Second half',
    driftPerBeat: (metres: string) => `${metres} m/beat`,
    driftHrSpeed: (hr: number, pace: string) => `${hr} bpm · ${pace}`,
    footnote: 'All conclusions compare you to your own history, never a population baseline, and none are medical judgements. Missing data reads "Not provided", never zero-filled.',
    notProvided: 'Not provided',
    durationHours: (hours: number, minutes: number) => `${hours} hr ${minutes} min`,
    durationMinutes: (minutes: number) => `${minutes} min`,
    metric: {
      'run.distance': 'Distance',
      'run.duration': 'Time',
      'run.pace': 'Avg pace',
      'run.avg_hr': 'Avg HR',
      'run.training_load': 'Training load',
    },
    confidence: {
      high: 'Well evidenced',
      medium: 'Some evidence',
      low: 'Thin evidence',
      insufficient: 'Not enough evidence',
    },
  },
);
