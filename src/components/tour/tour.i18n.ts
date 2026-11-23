import { defineMessages, useMessages } from '../../i18n';

const messages = defineMessages(
  {
    chapter: {
      intro: 'Getting started', home: 'Overview', metric: 'Metrics and picking days', workouts: 'Workouts', sleep: 'Sleep', ai: 'Send to AI',
      plan: 'Training plans', journal: 'Journal and rings', devices: 'Devices and data health', settings: 'Settings',
    },
    steps: {
      intro: {
        s1: 'Dark, light, or follow the system. Try it.',
        s2: 'Connect your Zepp account two ways: official authorization first, advanced data for what it lacks, like HRV, SpO₂ and stress.',
        s3: 'Your go-to metrics are pinned here; the line under each is your own usual range, not anyone else’s.',
        s4: 'Three tabs up top; sleep, workouts and your journal open from the cards on Overview.',
        s5: 'Sync status lives here; click to sync. A green ring on the top-left icon means an update is ready.',
        s6: 'Each section explains itself the first time you open it; come back to this card to see any of it again.',
      },
      home: {
        s1: 'Reorder the overview by what you care about. Try it.',
        s2: 'Every panel opens into a longer range and more detail.',
        s3: 'After a planned workout you sent to your watch, tell it here how it went.',
        s4: 'Things an AI wants to remember wait here until you say yes.',
        s5: 'Your journal and rings open from here.',
      },
      metric: {
        s1: 'Pick a time range once and every page remembers it.',
        s2: 'Tap “?” to see what this metric is and where it comes from, or leave it out of analysis.',
        s3: 'Pick a few days from here to hand to an AI.',
        s4: 'Tap a card to turn it over and tick it, or drag it straight into the box; tap empty space to put the cards away.',
      },
      workouts: {
        s1: 'One row per workout; open it for pace, heart rate and route, and to note how it felt.',
        s2: 'Select several to change their type or hide them; GPX, FIT and Strava imports start here too.',
      },
      sleep: { s1: 'One row per night; open it for stages, heart rate and when you woke.' },
      ai: {
        s1: 'Your picked data is on the left: tap a card to change how many days it covers, or flick it away to leave it out.',
        s2: 'Write your question on the right; it is packed into one .md for the AI.',
        s3: 'Hold the lock in the middle to hand it off: the file lands on your clipboard, so pressing Ctrl+V in the AI chat box attaches it.',
        s3Auto: 'Want the pasting and sending done for you too? Turn on “Hand off to AI automatically” in the AI tools card in Settings.',
        s4: 'Tools like Claude Code and Codex can also read it directly over MCP, no copy and paste.',
      },
      plan: {
        s1: "Paste the AI's final reply back and it becomes a week's plan.",
        s2: 'Tap a day to fine-tune it on its own.',
        s3: 'Hold to send it to your watch; when you start the workout there, you still pick the sport type once more.',
        s4: 'Plans you have sent are kept here.',
      },
      journal: {
        s1: 'A whole year of activity on one page; open the rings to watch them grow.',
        s2: 'Personal bests from 5K to the marathon.',
        s3: 'Race countdowns and your medal wall.',
        s4: 'Shoe mileage, with a reminder when it’s time to retire a pair.',
        s5: 'One ring a year, bulging in the weeks you trained more; tap a fine line to see that workout.',
      },
      devices: {
        s1: 'One row per device; open one to correct its model, or hide it. Its data stays either way.',
        s2: 'See how far each data stream has synced and what is missing; it lives in the Advanced card in Settings.',
      },
      settings: {
        s1: 'Settings is a stack of cards; tap one to open it.',
        s2: 'Backup, restore and history backfill live here.',
        s3: 'MCP lets tools like Claude Code and Codex read your data on this computer directly (read-only, nothing goes online); the command-line tool is here too.',
      },
    },
    next: 'Next', prev: 'Back', done: 'Done', skip: 'Skip guide', gotIt: 'Got it', tryIt: 'Try it inside the frame',
    progress: (index: number, total: number) => `${index} / ${total}`,
    sample: 'Sample data',
    sampleHint: 'The guide uses sample data; your library is untouched',
    welcomePick: 'Pick a language', welcomeStart: 'Show me around', welcomeSkip: 'Skip and start',
    inviteLater: 'Later',
    toolsText: 'Your AI tools still use the old Zebra; reconnect them once', toolsGo: 'Reconnect',
    cardTitle: 'Guide',
    seenCount: (seen: number, total: number) => `${seen} of ${total} seen`,
    start: 'Start', look: 'Show', seenMark: 'Seen',
    resetAll: 'Show all tips again', resetAllDone: 'Done; each section will explain itself again next time you open it',
  },
);

export const tourMessages = messages;
export const useTourText = () => useMessages(messages);
