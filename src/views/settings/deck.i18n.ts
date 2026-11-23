import { defineMessages } from '../../i18n';

export const deckMessages = defineMessages(
  {
    pageIntro: 'Open a card to adjust it; once open, drag its header sideways to flip to the next.',
    pageIntroDeck: 'Swipe sideways to pick a card, click the center one to open — or “Show all” to see everything.',
    openCard: 'Open',

    cardAccount: 'Account and devices',
    cardArchive: 'Archive and storage',
    cardData: 'Your data',
    cardAi: 'AI tools',
    cardDisplay: 'Display and language',
    cardPrivacy: 'Privacy and security',
    cardAdvanced: 'Advanced and maintenance',

    sumAccount: (state: string, devices: number) => `${state} · ${devices} ${devices === 1 ? 'device' : 'devices'}`,
    sumAccountOff: 'No Zepp account connected yet',
    sumSyncOn: (minutes: number) => `Auto sync · every ${minutes} min`,
    sumSyncOff: 'Auto sync is off',
    sumArchiveOn: 'Kept long-term · nothing pruned',
    sumArchiveOff: (days: number) => `Keeping the last ${days} days · older data pruned`,
    sumData: (available: number, total: number) => `${available} of ${total} data streams stored locally`,
    sumDataLoading: 'Checking available data…',
    sumAi: (format: string) => `MCP access · exports default to ${format}`,
    sumDisplay: (language: string, unit: string, scale: number) => `${language} · ${unit} · ${scale}%`,
    sumPrivacy: 'Stays on this computer by default',
    sumFeedback: 'Device not recognized or data looks wrong? Send us a redacted report',
    sumAdvanced: 'Backup and restore · local API · data health',
    autoSyncToggle: 'Auto sync',

    secAccount: 'Account',
    secDevices: 'Devices',
    secLogin: 'Sign-in method',
    secLoginSub: 'Expand only to switch sign-in method',
    secCapability: 'Data you have',
    secCodes: 'Unrecognized workout codes',
    secMcp: 'MCP',
    secExport: 'Default export',
    secFeedback: 'Report a problem',

    firstSyncing: (current: number, total: number) => `Connected · fetching recent records ${current}/${total}`,
    firstSyncingPlain: "Connected · fetching recent records",
    firstReady: "Your first data is in: see this week on the overview",
    goOverview: "Go to overview",
    deviceOpen: 'View or change model',


    mcpPreview: 'Show what will be copied',
    exportFormatSub: 'Preselected when exporting from “Send to AI” or a workout',

    themeLabel: 'Theme',
    themeSystem: 'System',
    themeDark: 'Dark',
    themeLight: 'Light',
    scaleSub: '100% is the design size; Ctrl + / Ctrl - also work',
    effectsLabel: 'Visual effects',
    effectsSub: 'Low turns off frosted glass and morphing animations; Auto uses Low on Linux, slower machines or low battery',
    effectsAuto: 'Auto',
    effectsFull: 'Full',
    effectsLite: 'Low',

  },
);
