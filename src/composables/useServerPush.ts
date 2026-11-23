import { ref } from 'vue';
import { tauriApi, toUserMessage } from './useTauriApi';
import { messagesOf } from '../i18n';
import type { PushSaveInput, PushState } from '../lib/bridge/types';
import { pushMessages } from '../views/settings/sections/push.i18n';

const state = ref<PushState | null>(null);
const busy = ref<string | null>(null);
const outcome = ref<{ ok: boolean; text: string } | null>(null);

let inflight: Promise<void> | null = null;
const refresh = (): Promise<void> => {
  inflight ??= tauriApi.pushState()
    .then((next) => { state.value = next; })
    .catch(() => { })
    .finally(() => { inflight = null; });
  return inflight;
};

const act = async (name: string, run: () => Promise<PushState>, done?: string): Promise<boolean> => {
  if (busy.value) return false;
  busy.value = name; outcome.value = null;
  try {
    state.value = await run();
    if (done) outcome.value = { ok: true, text: done };
    return true;
  } catch (cause) {
    outcome.value = { ok: false, text: toUserMessage(cause, messagesOf(pushMessages).failed) };
    void refresh();
    return false;
  } finally {
    busy.value = null;
  }
};

const run = () => act('run', () => tauriApi.pushRun());

const save = async (input: PushSaveInput) => {
  const ok = await act('save', () => tauriApi.pushSave(input));
  if (ok && state.value?.enabled) await run();
  return ok;
};
const importCompanion = async () => {
  const ok = await act('import', () => tauriApi.pushImportCompanion());
  if (ok) await run();
  return ok;
};
const setEnabled = (enabled: boolean) => act('toggle', () => tauriApi.pushSetEnabled(enabled));
const remove = (done: string) => act('remove', () => tauriApi.pushRemove(), done);

export const useServerPush = () => ({ state, busy, outcome, refresh, save, importCompanion, setEnabled, run, remove });
