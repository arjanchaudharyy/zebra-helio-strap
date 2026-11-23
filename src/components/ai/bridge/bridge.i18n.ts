import { defineMessages, useMessages } from '../../../i18n';

const messages = defineMessages(
  {
    past: 'Your past', future: 'Your next step', today: 'Today',
    days: (n: number) => `${n} days`, lookback: 'Lookback range', missing: 'No data',
    included: 'Included for AI', excluded: 'Excluded from AI', selected: (n: number) => `${n} workouts selected`, pickDays: (label: string) => `Pick ${label} days for the AI`, mean: 'Mean of recorded days',
    receive: 'Receive final plan', receiveEmpty: 'The clipboard does not contain a final plan. Copy the final reply containing plan JSON.',
    received: 'Final plan received', receiving: 'Receiving…', transcript: (n: number) => `${n} lines → a plan`,
    rest: 'Rest', bedtime: 'Bedtime', sleepTarget: 'Sleep target',
    hold: 'Hold · send to watch', holdHint: 'Hold for 0.6 seconds to stamp; Space / Enter also works',
    deleteDay: 'Remove this day', discard: 'Discard draft', detail: 'View this day', close: 'Close', undo: 'Undo',
    composer: 'What do you want to discuss with the AI?', placeholder: 'For example: keep one interval session next week, but Wednesday is busy. How should I arrange it?',
    sleepIntentHint: 'Sends the last 14 days of sleep and asks how you slept and what to improve', weekIntentHint: 'Sends the last 7 days and asks whether the load suits you and how recovery is trending', workoutIntentHint: 'Pick workouts under “Your past”, then ask whether intensity and recovery were right', nextIntentHint: 'Sends the last 30 days to plan next week together; the AI gives a plan only when you say “finalize”',
    sleepQuestion: 'How has my sleep been? What should I improve?', weekQuestion: 'Is this week’s training volume suitable? Is my recovery improving or worsening?',
    workoutQuestion: 'Analyze my selected workouts. Are the intensity and recovery arrangements appropriate?', nextQuestion: 'Use my recent training, sleep and recovery to discuss next week with me first. Only produce a plan when I say “finalize”.',
    profile: 'Personal profile', profileTitle: 'Give the AI your context', profileHint: 'Injuries, goals, recent state. Saved locally and included in future packages.',
    profilePlaceholder: 'For example: autumn half marathon, four runs a week; occasional right knee discomfort, Wednesdays unavailable.', save: 'Save profile', saving: 'Saving…',
    history: 'Round trips', historyHint: 'The data you sent, and the next step you brought back', emptyHistory: 'Your first round trip will live here',
    exported: 'File prepared', noPlan: 'No plan received yet', planBack: 'Plan received', delivered: 'Delivered', unknown: 'Unconfirmed', rejected: 'Rejected',
    historical: 'Viewing history · read only', undone: 'Undone', mcp: 'A plan arrived from MCP', accept: 'Accept this plan',
    previewPaste: 'Web preview: paste the final reply here', inspect: 'Receive and check', fullDemo: 'Full demo · simulated data only',
    demoHint: '90 days, food and weight, a two-week plan; no real account or watch', demoDelivered: 'Local simulation complete. Nothing was sent to a real watch.', nutrition: 'Food records',
    planned: 'Planned', actual: 'Actual', done: 'Matched workout', missed: 'No matching workout', extra: 'Extra workout', retry: 'Read again', unitMin: 'min',
  },
);
export const useBridgeText = () => useMessages(messages);
