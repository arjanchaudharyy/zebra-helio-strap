import { defineMessages, messagesOf } from '../../i18n';


const messages = defineMessages(
  {
    readinessWhat: 'A readiness score the watch works out from last night’s sleep, HRV and resting heart rate.',
    readinessChart: 'One point per day; the trend is your own ups and downs, never a comparison with anyone else.',
    readinessHow: 'The score is computed on the watch and arrives via advanced data — the official API has no readiness.',
    stressWhat: 'All-day stress: the watch measures about every five minutes and sums it up per day.',
    stressChart: 'This card shows the daily average, with the shaded band marking that day’s measured range; the last 24 hours, reading by reading, are in the curve card above.',
    stressHow: 'Via advanced data — the official API has no all-day stress. Time without readings stays blank rather than being filled with 0.',
    spo2What: 'Blood oxygen (SpO₂): how fully the haemoglobin in your blood is carrying oxygen, in %.',
    spo2Chart: 'Individual readings averaged into one point per day; the shaded band is that day’s measured range.',
    spo2How: 'Each reading is one measurement by the watch, via advanced data — the official API has no individual SpO₂.',
    spo2_odiWhat: 'Nighttime SpO₂ ODI: desaturations per hour on average; lower is better.',
    spo2_odiChart: 'One point per day from that night’s monitoring; nights without monitoring get no point and are never zero-filled.',
    spo2_odiHow: 'The watch’s own measure from nighttime SpO₂ monitoring, via advanced data — the official API has none of it, and it is never re-derived from sparse readings.',
    hrvWhat: 'Heart rate variability (the SDNN measure): how much the spacing between heartbeats varies, in ms.',
    hrvChart: 'Individual measurements averaged into one point per day; the shaded band is that day’s measured range.',
    hrvHow: 'Via advanced data. The official API also reports an HRV number, but its algorithm and unit are unpublished — it does not stand in for SDNN, so it is not collected here.',
    hrv_rmssdWhat: 'A different heart-rate-variability measure (RMSSD) — not the same number as SDNN.',
    hrv_rmssdChart: 'Nighttime measurements averaged into one point per day; the shaded band is that day’s measured range.',
    hrv_rmssdHow: 'Via advanced data; the official HRV number does not stand in for RMSSD either.',
    respiratory_rateWhat: 'Respiratory rate: breaths per minute while you sleep.',
    respiratory_rateChart: 'Averaged into one point per day; the shaded band is that day’s measured range.',
    respiratory_rateHow: 'Measured by the watch during sleep, via advanced data — the official API has no respiratory rate.',
    resting_hrWhat: 'Resting heart rate: beats per minute while at rest.',
    resting_hrChart: 'One value from the watch per day, joined into a trend; kept separate from the moment-to-moment heart rate of the last 24 hours.',
    resting_hrHow: 'Reported by the watch in its daily summary, synced via advanced data — not recomputed here.',
  },
);

export const VITALS_METRICS = [
  'readiness',
  'stress',
  'spo2',
  'spo2_odi',
  'hrv',
  'hrv_rmssd',
  'respiratory_rate',
  'resting_hr',
] as const;

export const vitalsInfo = (metric: string): { what: string; chart: string; how: string } | null => {
  if (!(VITALS_METRICS as readonly string[]).includes(metric)) return null;
  const t = messagesOf(messages) as Record<string, string>;
  return {
    what: t[`${metric}What`],
    chart: t[`${metric}Chart`],
    how: t[`${metric}How`],
  };
};
