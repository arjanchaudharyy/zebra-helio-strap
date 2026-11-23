import { defineMessages } from './i18n';

export const messages = defineMessages(
  {
    skipToContent: 'Skip to main content',
    mainNav: 'Main navigation',
    bottomNav: 'Mobile main navigation',
    navOverview: 'Overview',
    navHandoff: 'Send to AI',
    navSettings: 'Settings',
    preparingData: 'Opening your local database — the first launch after an update can take ten seconds or so…',
    compacting: (pending: number) =>
      `Compacting stored payloads (${pending} left) — clears itself when done; sync waits meanwhile.`,
    compacted: (saved: string) => `Stored payloads compacted — freed about ${saved} of disk.`,
    trayHint: 'Closing the window keeps ZeppBridge in the tray — auto-sync continues.',
    browserPreview: 'Use the desktop app. This browser preview reads no account data.',
    routeNotFound: 'Page not found — back to Overview.',
    quickReturn: (page: string) => `Back to ${page}`,
    previousPage: 'the previous page',
  },
);
