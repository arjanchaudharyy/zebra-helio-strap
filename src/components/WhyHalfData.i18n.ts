import { defineMessages } from '../i18n';

export const whyHalfDataMessages = defineMessages(
  {
    title: 'Why is some data missing?',
    lead: 'Zebra has two connections, and they don’t return the same data.',
    officialName: 'Official authorization',
    officialHas: 'Heart rate|Sleep|Steps|Runs and walks|PAI|Weight',
    officialLacks: 'Can’t backfill older history',
    advancedName: 'Advanced data',
    advancedHas: 'HRV|Blood oxygen|All-day stress|Readiness|Charge|Training load|Cycling and other workouts|Food logs|Device identification|Older history',
    advancedLogin: 'Zepp email or phone number + password only',
    advancedNoThirdParty: 'Google, Apple, WeChat, Xiaomi and other third-party sign-in don’t work',
    footer: 'With official authorization alone, nothing in the Advanced data column comes through. That’s a limit of what the official channel offers, not a Zebra bug.',
    close: 'Got it',
  },
);
