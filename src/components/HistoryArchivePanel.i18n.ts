import { defineMessages } from '../i18n';

export const archiveMessages = defineMessages(
  {
    autoContinueTitle: 'Start the next round automatically',
    estimateDetails: 'Per-stream breakdown',
    statPersisted: 'Saved',
    statEmpty: 'Empty in cloud',
    statPending: 'To do',
    statFailed: 'Failed',
    startLabel: 'Backfill from',
    startAria: 'History backfill start',
    customDateLabel: 'Start date',
    customDateAria: 'Backfill start date',
    estimateTitle: 'Estimated growth',
    estimateRate: (days: number, perDay: string) => `${days} days of local samples · about ${perDay}/day`,
    unmeasured: (streams: string) =>
      `Not enough local samples to estimate: ${streams}. Left out of the total above — better "unknown" than an invented rate multiplied by years.`,
    wouldBeCleanedUp: (requested: number, retention: number) =>
      `Backfilling ${requested} days, but this machine keeps only the last ${retention} — fetched data would be deleted at the next successful sync. Set "Keep data for" to Forever, or keep more days.`,
    backfilling: 'Backfilling…',
    continueBackfill: 'Continue backfilling',
    startBackfill: 'Start backfilling',
    autoContinue: 'Run to completion',
    autoContinueHint: 'Each round starts the next automatically until everything is backfilled. Stop any time — nothing already fetched is lost.',
    stopBackfill: 'Stop',
    stopping: 'Stopping…',
    roundProgress: (done: number, total: number) =>
      `Backfilling: ${done}/${total} monthly chunks done. Stop any time.`,
    stoppedByUser: (remaining: number) =>
      `Stopped with ${remaining} monthly chunks left. Everything fetched is kept — press "Continue backfilling" to carry on.`,
    stalled: (remaining: number) =>
      `${remaining} monthly chunks remain, but this round moved none, so it stopped. Most likely they keep failing — see the failed list below, or press "Retry failed items".`,
    deferredRetry: 'Local maintenance running — backfill continues automatically',
    resetLedger: 'Clear ledger',
    ledgerTitle: 'Coverage ledger',
    ledgerProgress: (done: number, total: number) => `${done} of ${total} monthly chunks resolved`,
    ledgerFrom: (from: string) => ` · requested from ${from}`,
    ledgerComplete: 'Every monthly chunk is resolved: written locally, or the cloud plainly has nothing for that period.',
    ledgerIncomplete: (remaining: number) =>
      `${remaining} chunks unresolved. Until all are done, this local copy covers only the successfully synced range — not a complete copy.`,
    ledgerRange: (from: string, to: string, records: number) => `${from} ~ ${to} · ${records} records`,
    ledgerNothingWritten: 'No month written yet',

    range1y: 'Last 1 year',
    range2y: 'Last 2 years',
    range3y: 'Last 3 years',
    rangeAll: (years: number) => `All available history (up to ${years} years)`,
    rangeCustom: 'Custom start',

    confirmDisableArchive: 'Keeping a fixed number of days means the next successful sync deletes older data — irreversible.\nIf you just backfilled, take a database snapshot first.\nChange it?',
    archiveEnabled: 'Keeping data forever: syncs no longer delete anything.',
    archiveDisabled: 'Keeping a fixed number of days: the next successful sync deletes older data.',
    archiveSaveFailed: 'Could not save archive setting',
    pickStartFirst: 'Pick a backfill start first.',
    outOfRetention: 'This backfill reaches past the local retention window — fetched data would be pruned at the next successful sync. Set "Keep data for" to Forever, or keep more days.',
    roundDone: (remaining: number) =>
      `Round done; ${remaining} monthly chunks left. Press "Continue backfilling" to carry on — stop any time.`,
    allChunksDone: 'Every monthly chunk in the ledger is resolved.',
    backfillFailed: 'History backfill failed',
    confirmResetLedger: 'Clears the coverage ledger only — nothing already written is deleted. You can plan a new backfill afterwards. Continue?',
    ledgerReset: 'Ledger cleared. Plan a new backfill range.',
    ledgerResetFailed: 'Could not clear ledger',
    failedTitle: 'Months that could not be fetched',
    failedIntro: 'These chunks failed; all other months were backfilled as usual.',
    failedRow: (stream: string, month: string) => `${stream} · ${month}`,
    failedAttempts: (attempts: number) => `${attempts} attempt${attempts === 1 ? '' : 's'}`,
    failedExhausted: 'Automatic retries used up — press "Retry failed items" to try again',
    retryFailed: 'Retry failed items',
    retryFailedDone: 'Failed months re-queued. Continue the backfill.',
    retryFailedFailed: 'Could not re-queue failed months',
    streamSeparator: ', ',

    stream: {
      heart_rate: 'Heart rate',
      daily_summary: 'Daily summaries',
      workouts: 'Workouts',
      sleep: 'Sleep',
      hrv: 'Heart rate variability',
      wellness: 'Stress / SpO2 and similar',
    },
  },
);
