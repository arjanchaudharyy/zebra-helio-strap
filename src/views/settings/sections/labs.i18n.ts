import { defineMessages } from '../../../i18n';

export const labsMessages = defineMessages(
  {
    cardTitle: 'Beta lab',
    summary: 'Features still being tried out, off by default',
    lead: 'Everything here is experimental: it may change or be removed. If something breaks, write to the author from the letter box.',
    badge: 'Experimental',
    unstrapImportTitle: 'Import workouts from unstrap',
    unstrapImportSub: 'Pick an unstrap backup zip: the workouts the strap and the phone recorded (heart rate, route, pauses) come in, one per session, along with any heart rate, SpO₂, resting heart rate and sleep not already on this computer. Nothing synced from Zepp is changed. Scores unstrap works out itself are left out. Imports are listed on the Workouts page, where each can be undone.',
    unstrapImportButton: 'Choose backup',
    unstrapExportTitle: 'Export for unstrap',
    unstrapExportSub: 'Writes the workouts on this computer as a zip unstrap can import (each workout’s summary and heart rate); pick it under Restore from backup in unstrap. Missing values stay blank, never 0. Zepp’s training load and VO₂max are left out.',
    unstrapExportButton: 'Export',
    unstrapRange: 'Range',
    unstrapRangeDays: (n: number) => `Last ${n} days`,
    unstrapRangeAll: 'All',
    unstrapExported: (workouts: number) => `Done: ${workouts} workouts`,
    unstrapShow: 'Show in folder',
  },
);
