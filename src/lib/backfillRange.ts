import { localDateString } from './format';

export type BackfillChoice = '1y' | '2y' | '3y' | 'all' | 'custom';

export const HISTORY_LIMIT_DAYS = 3650;

export const ALL_HISTORY_DAYS = HISTORY_LIMIT_DAYS - 2;

export const ALL_HISTORY_YEARS = 10;

const daysBefore = (today: Date, days: number): string => {
  const date = new Date(today.getFullYear(), today.getMonth(), today.getDate() - days);
  return localDateString(date);
};

const yearsBefore = (today: Date, years: number): string => {
  const date = new Date(today);
  date.setFullYear(date.getFullYear() - years);
  return localDateString(date);
};

export const earliestBackfillDate = (today: Date): string => daysBefore(today, ALL_HISTORY_DAYS);

export const defaultCustomFrom = (today: Date): string => yearsBefore(today, 1);

export const backfillStartDate = (choice: BackfillChoice, today: Date, custom: string): string => {
  switch (choice) {
    case '1y': return yearsBefore(today, 1);
    case '2y': return yearsBefore(today, 2);
    case '3y': return yearsBefore(today, 3);
    case 'all': return earliestBackfillDate(today);
    default: {
      if (!/^\d{4}-\d{2}-\d{2}$/.test(custom)) return '';
      const earliest = earliestBackfillDate(today);
      return custom < earliest ? earliest : custom;
    }
  }
};
