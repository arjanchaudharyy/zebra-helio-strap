import { defineMessages, messagesOf } from '../i18n';

export const RANGE_LADDER_DAYS = [7, 30, 90, 180, 365] as const;

export type RangeDays = (typeof RANGE_LADDER_DAYS)[number];

const messages = defineMessages(
  {
    d7: '7 days',
    d30: '1 month',
    d90: '3 months',
    d180: '6 months',
    d365: '1 year',
  },
);

const copy = () => messagesOf(messages);

export const rangeLabel = (days: RangeDays): string => {
  const t = copy();
  const labels: Record<RangeDays, string> = {
    7: t.d7,
    30: t.d30,
    90: t.d90,
    180: t.d180,
    365: t.d365,
  };
  return labels[days];
};

export const rangeOptions = (
  days: readonly RangeDays[],
): Array<{ days: RangeDays; label: string }> =>
  days.map((value) => ({ days: value, label: rangeLabel(value) }));

export const DISPLAY_RANGE_DAYS: readonly RangeDays[] = [7, 30, 180];
