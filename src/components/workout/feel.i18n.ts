import { defineMessages, useMessages } from '../../i18n';

const messages = defineMessages(
  {
    title: 'How it felt',
    rpe: 'Effort (RPE)',
    rpeEasy: 'Easy', rpeHard: 'Hard', rpeMax: 'All-out',
    watchRpe: (value: number) => `Watch said ${value}`,
    clear: 'Clear',
    soreness: 'Sore spots',
    markSore: 'Mark sore spots',
    fold: 'Done',
    shoe: 'Shoes worn',
    auto: 'Auto',
    pick: 'Pick a pair',
    none: 'Not a logged pair',
    how: { chosen: 'your pick', zepp: 'recorded by Zepp', default: 'default pair', none: 'you said not a logged pair', unknown: 'unmatched' },
    autoNow: (name: string, how: string) => `Counting toward “${name}” (${how})`,
    noShoes: 'No shoes logged yet.',
    register: 'Add them in Journal',
    retired: 'Retired',
    saved: 'Saved',
  },
);

export const useFeelText = () => useMessages(messages);
