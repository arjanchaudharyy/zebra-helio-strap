import { defineMessages, messagesOf } from '../../i18n';

const messages = defineMessages({
  systolic: 'Systolic pressure during heart contraction, in mmHg.',
  diastolic: 'Diastolic pressure between heartbeats, in mmHg.',
  pulse: 'Pulse returned with a blood-pressure measurement, in bpm. Stored separately from all-day heart rate.',
  chart: 'Bars show daily measured averages; minimum and maximum come from individual readings. Unmeasured dates stay blank. Exports retain each measurement time and value.',
  how: 'Experimental blood-pressure sync accepts explicit paired readings. Candidate endpoints still need verification on a measuring account. Method and calibration metadata remain in the raw response.',
});

export const BLOOD_PRESSURE_METRICS = ['blood_pressure_systolic', 'blood_pressure_diastolic', 'blood_pressure_pulse'];
export const bloodPressureInfo = (metric: string) => {
  if (!BLOOD_PRESSURE_METRICS.includes(metric)) return null;
  const t = messagesOf(messages);
  return { what: metric.endsWith('_systolic') ? t.systolic : metric.endsWith('_diastolic') ? t.diastolic : t.pulse, chart: t.chart, how: t.how };
};
