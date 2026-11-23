import { defineMessages } from '../../i18n';

export const workoutClimbsMessages = defineMessages(
  {
    eyebrow: 'Climbs',
    title: 'Main climbs and descents',
    aria: 'Main climb and descent segments',
    flat: 'No notable climbs: nothing reached the elevation and grade thresholds below.',
    climb: 'Climb',
    descent: 'Descent',
    range: (from: string, to: string) => `${from}–${to}`,
    grade: 'Grade',
    vertical: 'Vertical speed',
    heartRate: 'Heart rate',
    pace: 'Pace',
    speed: 'Speed',
    perHour: (unit: string) => `${unit}/h`,
    howTitle: 'How this is worked out',
    how: (window: number, reversal: number, change: number, grade: number, moving: number) =>
      `Altitude is smoothed with a ${window}-second moving average; a segment ends once it drops (or rises) more than ${reversal} m from its highest (lowest) point. ` +
      `Only segments with at least ${change} m of elevation change and an average grade of at least ${grade}% are listed. Distance comes from integrating per-point speed, calibrated to the recorded total distance; ` +
      `vertical speed and pace use moving time (time below ${moving} m/s is left out).`,
  },
);
