import type { CloudCompanionState } from './bridge/types';

export type CloudTone = 'ok' | 'warn' | 'idle';

export interface CloudStatusText {
  checking: string;
  unsupported: string;
  notInstalled: string;
  configBroken: string;
  notConfigured: string;
  running: string;
  neverRan: string;
  result: Record<string, string>;
  resultUnknown: string;
  statusAt: (text: string, when: string) => string;
}

export interface CloudStatus {
  tone: CloudTone;
  text: string;
  line: string;
}

const QUIET_RESULTS = new Set(['ok', 'skipped_unchanged', 'skipped_busy', 'skipped_locked']);

export const cloudStatus = (
  state: CloudCompanionState | null,
  t: CloudStatusText,
  formatWhen: (at: string) => string | null,
): CloudStatus => {
  const plain = (tone: CloudTone, text: string): CloudStatus => ({ tone, text, line: text });
  if (!state) return plain('idle', t.checking);
  if (!state.supported) return plain('idle', t.unsupported);
  if (state.config_unreadable) return plain('warn', t.configBroken);
  if (!state.configured) return plain(state.uploader_dir ? 'warn' : 'idle', state.uploader_dir ? t.notConfigured : t.notInstalled);
  if (state.running) return plain('ok', t.running);
  const run = state.last_run;
  if (!run) return plain('idle', t.neverRan);
  const text = Object.prototype.hasOwnProperty.call(t.result, run.result) ? t.result[run.result] : t.resultUnknown;
  const when = formatWhen(run.at);
  return {
    tone: QUIET_RESULTS.has(run.result) ? 'ok' : 'warn',
    text,
    line: when ? t.statusAt(text, when) : text,
  };
};
