import type { InsightFact } from '../types';

export type ChangeTone = 'good' | 'bad' | 'neutral' | 'flat';

const LOWER_IS_BETTER = new Set([
  'weekly.resting_hr',
  'weekly.stress',
  'weekly.sleep_start_regularity',
  'run.pace',
  'run.avg_hr',
]);
const HIGHER_IS_BETTER = new Set(['weekly.hrv']);

export const changeTone = (fact: Pick<InsightFact, 'fact_id' | 'comparison'>): ChangeTone => {
  const direction = fact.comparison?.direction;
  if (!direction || direction === 'same') return 'flat';
  const lower = direction === 'lower';
  if (LOWER_IS_BETTER.has(fact.fact_id)) return lower ? 'good' : 'bad';
  if (HIGHER_IS_BETTER.has(fact.fact_id)) return lower ? 'bad' : 'good';
  return 'neutral';
};

export const changeArrow = (fact: Pick<InsightFact, 'comparison'>): string => {
  const direction = fact.comparison?.direction;
  return direction === 'higher' ? '↑' : direction === 'lower' ? '↓' : '';
};

export type ReportTone = 'good' | 'bad' | 'up' | 'down';
export const reportTone = (fact: Pick<InsightFact, 'fact_id' | 'comparison'>): ReportTone => {
  const tone = changeTone(fact);
  if (tone === 'good' || tone === 'bad') return tone;
  return fact.comparison?.direction === 'lower' ? 'down' : 'up';
};
