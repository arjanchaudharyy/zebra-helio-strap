import { defineMessages, useMessages } from '../../i18n';

const messages = defineMessages(
  {
    draft: 'Draft not sent',
    delivered: 'Delivered',
    partial: 'Partly delivered',
    unconfirmed: 'Unconfirmed',
    rejected: 'Not delivered',
    sending: 'Sending…',
    title: 'Last sent to the watch',
    sentAt: (when: string) => `Sent ${when}`,
    demo: 'Simulated',
    never: 'Nothing sent yet',
    undo: 'Undo last send',
    clear: 'Clear watch plan',
    resend: 'Resend unconfirmed weeks',
  },
);

export const useStatusText = () => useMessages(messages);
