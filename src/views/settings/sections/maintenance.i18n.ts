import { defineMessages } from '../../../i18n';

export const maintenanceMessages = defineMessages(
  {
    secMaintenance: 'Maintenance',
    cleanupTitle: 'Prune by retention',
    cleanupSub: (date: string) => `Deletes data older than ${date}`,
    cleanupForever: 'Keeping everything; nothing to prune',
    reparseTitle: 'Reparse local data',
    reparseSub: 'Recomputes with current rules, offline',
    resetTitle: 'Clear coverage ledger',
    resetSub: 'Clears backfill records only; saved data stays',
  },
);
