import { defineMessages, messagesOf } from '../../i18n';

const isoDay = (date: Date, plus: number): string => {
  const moved = new Date(date.getFullYear(), date.getMonth(), date.getDate() + plus);
  return `${moved.getFullYear()}-${String(moved.getMonth() + 1).padStart(2, '0')}-${String(moved.getDate()).padStart(2, '0')}`;
};

const exampleFor = (now: Date, text: ExampleText): string => JSON.stringify({
  format: 'zeppbridge-plan/3',
  summary: text.summary,
  rest: [{ date: isoDay(now, 1), bedtime: '22:30', sleepTarget: '8h30m', note: text.restNote }],
  from: isoDay(now, 1),
  to: isoDay(now, 4),
  workouts: [
    {
      date: isoDay(now, 2),
      sport: 'running',
      variant: 'track',
      name: text.intervalName,
      focus: text.intervalFocus,
      description: text.intervalDescription,
      steps: [
        { kind: 'warmup', duration: '12min', target: 'hr 110-130' },
        { repeat: 3, steps: [{ kind: 'interval', duration: '1km', target: 'pace 4:10-4:20' }, { kind: 'recovery', duration: '2min' }] },
        { kind: 'cooldown', duration: '8min', target: 'hr 100-125' },
      ],
    },
    {
      date: isoDay(now, 4),
      sport: 'cycling',
      variant: 'outdoor',
      name: text.rideName,
      focus: text.rideFocus,
      description: text.rideDescription,
      steps: [{ kind: 'active', duration: '90min', target: 'hr 120-140' }],
    },
  ],
});

type ExampleText = {
  summary: string;
  restNote: string;
  intervalName: string;
  intervalFocus: string;
  intervalDescription: string;
  rideName: string;
  rideFocus: string;
  rideDescription: string;
};

const messages = defineMessages(
  {
    example: {
      summary: 'One quality session; the rest is aerobic work and recovery',
      restNote: 'Early night before the quality session',
      intervalName: '3×1km repeats',
      intervalFocus: 'Speed endurance',
      intervalDescription: 'Run every rep evenly, do not sprint the last one\nJog or walk the recoveries',
      rideName: 'Long easy ride',
      rideFocus: 'Build endurance',
      rideDescription: 'Cadence 85–95; do not chase heart rate on climbs',
    },
    guide: (example: string) => [
      'Discuss the plan with me first, across multiple rounds if needed. Only output final plan JSON when I explicitly say “finalize” or “send the plan”:',
      'Add a ```json code block to your reply in the format below. ZeppBridge reads it, I review it, and then it is sent to my watch.',
      '- date is YYYY-MM-DD, today or later, at most 56 days ahead. from / to is the range this plan covers: days in that range with no workout are treated as rest days and any existing plan on them is cleared. If omitted, it spans the earliest to the latest workout date.',
      '- The watch only accepts these 4 sports: running (variant: outdoor / treadmill / track / trail; the watch has no trail run to pick, so trail tells me to start it as a run), cycling (variant: outdoor / indoor), pool_swim and open_water_swim (no variant for swimming). The watch asks me to pick the sub type again, so get variant right.',
      '- Walking, hiking, strength, yoga and similar activities cannot be sent to the watch (Zepp silently drops them). Tell me during the discussion that such a session “cannot go to the watch” and let me decide whether to swap it for a sport above or leave it out. Never hide it in a rest note.',
      '- Every workout needs focus (its purpose, a short phrase such as “Build endurance” or “Threshold”) and description (the key points in a sentence or two; \\n for a new line). Both appear on the watch, with “purpose · sub type” as the first line.',
      '- Keep name within about 14 characters; longer names get cut off in the watch list.',
      '- Each step\'s kind is one of warmup, active, interval, recovery, rest, cooldown; duration is time (20min, 90s, 1h) or distance (400m, 5km); target is heart rate hr 135-150, pace pace 5:00-5:30 (per km), power power 200-220, or omitted (no target).',
      '- A repeat group is {"repeat": count, "steps": [...]}, count 1 to 50, and it cannot contain another repeat. A workout has at most 40 steps and lasts at most 8 hours. Several workouts on one day are fine.',
      '- format is zeppbridge-plan/3; summary is one sentence; rest is an array of local rest/sleep advice with date, bedtime (HH:MM), sleepTarget (8h30m) and note. Rest advice stays local and is never sent to the watch; it may coexist with workouts.',
      'Example:',
      '```json',
      example,
      '```',
    ].join('\n'),
    swim: '- Swim distances are in metres; each pool step is at least 25 m (ideally a whole number of pool lengths, which are set on the watch). Swims take no pace target: use a heart-rate range or none.',
    language: (lang: string) => `- Write name, focus, description, summary and note in ${lang}: they are shown as-is in ZeppBridge and on the watch.`,
  },
);

export const planGuide = (now: Date = new Date()): string => {
  const text = messagesOf(messages);
  return `${text.guide(exampleFor(now, text.example))}\n${text.swim}\n${text.language('English')}`;
};
