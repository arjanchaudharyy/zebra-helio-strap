import { defineMessages, useMessages } from '../../i18n';

const messages = defineMessages(
  {
    ask: 'Ask AI',
    askTitle: (label: string) => `Ask AI · ${label}`,
    askHint: 'Pick a question to start with: it opens a new task with just this metric’s recent data and the question filled in. You can still edit it before sending.',
    firstAbove: (label: string) => `${label} has been higher than usual lately. What might it mean?`,
    firstBelow: (label: string) => `${label} has been lower than usual lately. What might it mean?`,
    firstUsual: (label: string) => `Is the recent change in ${label} normal?`,
    withTraining: (label: string) => `Does ${label} line up with my recent training load?`,
    withSleep: 'Look at my sleep too',
    sleepWithTraining: 'Look at my sleep together with training',
    trainingNext: 'In this state, should next week go up or down?',
    bodyFood: (label: string) => `Does the change in ${label} match my food and training?`,
    workoutHint: 'Pick a question to start: it opens a new task with this workout and the days around it, with the question filled in. You can still edit it before sending.',
    workoutReview: (label: string) => `How did this ${label} go?`,
    workoutRecovery: (label: string) => `Did I recover enough after this ${label}?`,
    workoutNext: (label: string) => `How should I plan my next ${label}?`,
    sleepHint: 'Pick a question to start: it opens a new task with this night and the two weeks of sleep and recovery before it, with the question filled in. You can still edit it before sending.',
    sleepNight: 'How did I sleep this night?',
    sleepRecovery: 'Did this night of sleep get me recovered?',
    above: (delta: string) => `Above usual by ${delta}`,
    below: (delta: string) => `Below usual by ${delta}`,
    baselineHint: (median: string, days: number) => `Compared with you: the median of the previous ${days} days is ${median}. Not a diagnosis.`,
    usualWord: 'As usual',
    usualRange: (range: string) => `Usual ${range}`,
    usualHint: (range: string, days: number) => `Compared only with you: over the previous ${days} days, the middle 80% of days fell within ${range}. Not a diagnosis.`,
    longTermHint: (range: string, since: string, days: number, recent: string) =>
      `Your long-term norm: across ${days} days with readings since ${since}, the middle 80% fell within ${range}${recent ? `; the last 90 days were ${recent}` : ''}. Compared only with you, not a diagnosis.`,
  },
);

export const useAskText = () => useMessages(messages);
