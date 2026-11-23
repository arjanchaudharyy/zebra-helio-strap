import { defineMessages, useMessages } from '../i18n';

const messages = defineMessages(
  {
    offer: (date: string) => `This computer only goes back to ${date}. Backfill earlier history from the cloud?`,
    years: (n: number) => (n === 1 ? '1 year' : `${n} years`),
    later: 'Not now',
    running: 'Backfilling in the background — it keeps going if you leave this page',
    stop: 'Stop',
    settings: 'Backfill settings',
  },
);

export const useBackfillOfferText = () => useMessages(messages);
