import { defineMessages, messagesOf } from '../i18n';

const messages = defineMessages(
  {
    nickname: 'Demo user',
    lifeEvent: 'Started half-marathon training',
    noteMine: 'Swims on Wednesday evenings; no runs that day',
    noteProposed: 'Left knee hurts past 15 km; split long runs',
    taskTitle: (month: number, day: number) => `Last 14 days · ${month}/${day}`,
    taskFile: 'last 14 days',
    easyRun: 'Easy run',
    recoveryRide: 'Recovery ride',
    tempoRun: 'Tempo run',
    intervals: 'Intervals 5×3 min',
    longRun: 'Long slow run',
    shoeDaily: 'Daily trainers',
    shoeRace: 'Carbon racers',
    raceHalf: 'City Half Marathon',
    race10k: 'Autumn 10K road race',
    injury: 'Outer right knee pain (IT band)',
    capsuleNote: 'From a year ago: I had just started running three times a week and my knee still hurt a little. I hope you are still running, and still stretching.',
    codeLabel: 'Pickleball',
    lifeTrip: 'Business trip (Shanghai)',
    lifeCold: 'Cold',
    lifeShift: 'Moved to the early shift, up at 6',
    planSummary: 'Recovery week: volume down 20%, Saturday long run cut to 12 km',
    assistedBackExtension: 'Assisted Back Extension',
  },
);

export const demoText = () => messagesOf(messages);
