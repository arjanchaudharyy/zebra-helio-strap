import { defineMessages } from '../../i18n';

export const elevationFactsMessages = defineMessages(
  {
    aria: 'Elevation overview',
    ascent: 'Ascent',
    descent: 'Descent',
    drop: 'Elevation span',
    range: 'Altitude range',
    slopeTitle: 'Time by slope',
    up: 'Uphill',
    flat: 'Flat',
    down: 'Downhill',
    how: (grade: number, step: number, smooth: number, moving: number) =>
      `Time by slope: altitude is smoothed with a ${smooth}-second moving average, the grade is taken every ${step} m, and anything steeper than ±${grade}% counts as uphill or downhill, the rest as flat; time below ${moving} m/s is left out.`,
  },
);
