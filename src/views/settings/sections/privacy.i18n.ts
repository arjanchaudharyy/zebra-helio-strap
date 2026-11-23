import { defineMessages } from '../../../i18n';

export const privacyCardMessages = defineMessages(
  {
    lead: 'Stays on this computer by default, no usage statistics; it only ever goes to a site you set up yourself',
    learnMore: 'Learn more',
    apiOn: (url: string) => `On · ${url}`,
    apiOff: 'Off',
    apiOpen: 'Settings',
  },
);
