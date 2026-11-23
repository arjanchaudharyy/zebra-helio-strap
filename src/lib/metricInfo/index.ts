import { vitalsInfo, VITALS_METRICS } from './vitals';
import { bloodPressureInfo, BLOOD_PRESSURE_METRICS } from './bloodPressure';
import { bodyInfo, BODY_METRICS } from './body';
import { activityInfo, ACTIVITY_METRICS } from './activity';
import { heartInfo, HEART_METRICS } from './heart';
import { trainingInfo, TRAINING_METRICS } from './training';
import { sleepInfo, SLEEP_METRICS } from './sleep';


export interface MetricInfo {
  what: string;
  chart: string;
  how: string;
}

const LOOKUPS: ((metric: string) => MetricInfo | null)[] = [
  bloodPressureInfo,
  vitalsInfo,
  bodyInfo,
  activityInfo,
  heartInfo,
  trainingInfo,
  sleepInfo,
];

export const metricInfo = (metric: string): MetricInfo | null => {
  for (const lookup of LOOKUPS) {
    const info = lookup(metric);
    if (info) return info;
  }
  return null;
};

export const METRIC_INFO_IDS: string[] = [
  ...BLOOD_PRESSURE_METRICS,
  ...VITALS_METRICS,
  ...BODY_METRICS,
  ...ACTIVITY_METRICS,
  ...HEART_METRICS,
  ...TRAINING_METRICS,
  ...SLEEP_METRICS,
];
