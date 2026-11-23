import { defineMessages } from '../../../i18n';

export const syncCardMessages = defineMessages(
  {
    cardTitle: 'Sync',
    cardTitleUpdates: 'Sync and updates',
    autoSub: 'Fetches new data at this interval while the app is open',
    off: 'Off',
    lastSynced: (when: string) => `Last synced ${when}`,
    neverSynced: 'Not synced yet',
    newest: (when: string) => `Newest record ${when}`,
    cloudStale: (when: string) => `The cloud only has data up to ${when} · pull to refresh in the Zepp phone app first`,
    cloudStaleNoTime: 'No new data in the cloud yet · pull to refresh in the Zepp phone app first',
  },
);
