import { metricColor } from '../../lib/metricTone';
import { isFiniteNumber } from '../../lib/format';
import {
  bodyHeightUnitLabel,
  bodyMassUnitLabel,
  toBodyHeight,
  toBodyMass,
} from '../../lib/units';
import type { MetricSeries, MetricSeriesPoint } from '../../types';
import type { bodyStatusMessages } from '../BodyStatus.i18n';

type BodyStatusText = typeof bodyStatusMessages.en;

export interface BodyCard {
  metric: string;
  label: string;
  color: string;
  unit: string;
  decimals?: number;
  showSpread?: boolean;
  emptyText?: string;
  chart?: 'line' | 'bar';
  calendarAxis?: boolean;
  convert?: (value: number) => number;
}

/**
 * Everything on this screen already sits in the local library — this page is
 * presentation, not collection. The list is fixed so the backend can refuse
 * any name it does not have a unit for.
 */
const VITALS_METRICS = [
  'blood_pressure_systolic',
  'blood_pressure_diastolic',
  'blood_pressure_pulse',
  'readiness',
  'stress',
  'spo2',
  'spo2_odi',
  'hrv',
  'hrv_rmssd',
  'respiratory_rate',
  'resting_hr',
];

const BODY_METRICS = [
  'weight',
  'bmi',
  'body_fat_rate',
  'muscle_mass',
  'body_water_rate',
  'bone_mass',
  'visceral_fat',
  'bmr',
  'height',
];

const INTAKE_METRICS = ['intake_calories', 'intake_protein_g', 'intake_fat_g', 'intake_carbs_g'];

export const METRICS = [...VITALS_METRICS, ...BODY_METRICS, ...INTAKE_METRICS];

export type CardGroup = 'vitals' | 'body' | 'intake';

export const groupOf = (metric: string): CardGroup => {
  if (INTAKE_METRICS.includes(metric)) return 'intake';
  if (BODY_METRICS.includes(metric)) return 'body';
  return 'vitals';
};

export const buildBodyCards = (t: BodyStatusText): BodyCard[] => [
  { metric: 'blood_pressure_systolic', label: t.systolicLabel, color: metricColor('blood_pressure_systolic'), unit: 'mmHg', showSpread: true, chart: 'bar', calendarAxis: true },
  { metric: 'blood_pressure_diastolic', label: t.diastolicLabel, color: metricColor('blood_pressure_diastolic'), unit: 'mmHg', showSpread: true, chart: 'bar', calendarAxis: true },
  { metric: 'blood_pressure_pulse', label: t.bloodPressurePulseLabel, color: metricColor('blood_pressure_pulse'), unit: 'bpm', showSpread: true, chart: 'bar', calendarAxis: true },
  {
    metric: 'readiness',
    label: t.readinessLabel,
    color: metricColor('readiness'),
    unit: t.unitScore,
  },
  {
    metric: 'stress',
    label: t.stressLabel,
    color: metricColor('stress'),
    unit: t.unitScore,
    showSpread: true,
  },
  {
    metric: 'spo2',
    label: t.spo2Label,
    color: metricColor('spo2'),
    unit: '%',
    showSpread: true,
    emptyText: t.spo2Empty,
  },
  {
    metric: 'spo2_odi',
    label: t.odiLabel,
    color: metricColor('spo2_odi'),
    unit: t.unitPerHour,
    decimals: 1,
  },
  {
    metric: 'hrv',
    label: 'HRV (SDNN)',
    color: metricColor('hrv'),
    unit: 'ms',
    showSpread: true,
  },
  {
    metric: 'hrv_rmssd',
    label: 'HRV (RMSSD)',
    color: metricColor('hrv_rmssd'),
    unit: 'ms',
    showSpread: true,
  },
  {
    metric: 'respiratory_rate',
    label: t.respiratoryLabel,
    color: metricColor('respiratory_rate'),
    unit: t.unitBreathsPerMinute,
    decimals: 1,
    showSpread: true,
  },
  {
    metric: 'resting_hr',
    label: t.restingLabel,
    color: metricColor('resting_hr'),
    unit: 'bpm',
  },
  {
    metric: 'weight',
    label: t.weightLabel,
    color: metricColor('weight'),
    unit: bodyMassUnitLabel(),
    decimals: 1,
    showSpread: true,
    emptyText: t.scaleEmpty,
    convert: toBodyMass,
  },
  {
    metric: 'bmi',
    label: t.bmiLabel,
    color: metricColor('bmi'),
    unit: '',
    decimals: 1,
    emptyText: t.scaleEmpty,
  },
  {
    metric: 'body_fat_rate',
    label: t.fatLabel,
    color: metricColor('body_fat_rate'),
    unit: '%',
    decimals: 1,
    showSpread: true,
  },
  {
    metric: 'muscle_mass',
    label: t.muscleLabel,
    color: metricColor('muscle_mass'),
    unit: bodyMassUnitLabel(),
    decimals: 1,
    convert: toBodyMass,
  },
  {
    metric: 'body_water_rate',
    label: t.waterLabel,
    color: metricColor('body_water_rate'),
    unit: '%',
    decimals: 1,
  },
  {
    metric: 'bone_mass',
    label: t.boneLabel,
    color: metricColor('bone_mass'),
    unit: bodyMassUnitLabel(),
    decimals: 2,
    convert: toBodyMass,
  },
  {
    metric: 'visceral_fat',
    label: t.visceralLabel,
    color: metricColor('visceral_fat'),
    unit: t.unitGrade,
  },
  {
    metric: 'bmr',
    label: t.bmrLabel,
    color: metricColor('bmr'),
    unit: t.unitKcalPerDay,
  },
  {
    metric: 'height',
    label: t.heightLabel,
    color: metricColor('height'),
    unit: bodyHeightUnitLabel(),
    decimals: 1,
    convert: toBodyHeight,
  },
  {
    metric: 'intake_calories',
    label: t.intakeCaloriesLabel,
    color: metricColor('intake_calories'),
    unit: t.unitKcal,
    chart: 'bar',
    calendarAxis: true,
  },
  {
    metric: 'intake_protein_g',
    label: t.proteinLabel,
    color: metricColor('intake_protein_g'),
    unit: t.unitGram,
    chart: 'bar',
    calendarAxis: true,
  },
  {
    metric: 'intake_fat_g',
    label: t.fatIntakeLabel,
    color: metricColor('intake_fat_g'),
    unit: t.unitGram,
    chart: 'bar',
    calendarAxis: true,
  },
  {
    metric: 'intake_carbs_g',
    label: t.carbsLabel,
    color: metricColor('intake_carbs_g'),
    unit: t.unitGram,
    chart: 'bar',
    calendarAxis: true,
  },
];

export const convertSeries = (
  source: MetricSeries | null,
  convert?: (value: number) => number,
): MetricSeries | null => {
  if (!source || !convert) return source;
  const num = (value: number | null | undefined): number | null | undefined =>
    isFiniteNumber(value) ? convert(value) : value;
  const point = (item: MetricSeriesPoint): MetricSeriesPoint => ({
    ...item,
    value: convert(item.value),
    min: num(item.min),
    max: num(item.max),
  });
  return {
    ...source,
    points: source.points.map(point),
    latest: source.latest ? point(source.latest) : source.latest,
    average: num(source.average) ?? null,
    minimum: num(source.minimum) ?? null,
    maximum: num(source.maximum) ?? null,
  };
};

