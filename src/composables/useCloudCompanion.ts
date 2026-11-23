import { computed, ref, watch } from 'vue';
import { tauriApi, toUserMessage } from './useTauriApi';
import { messagesOf, useMessages } from '../i18n';
import { labFlag } from '../lib/labs';
import { cloudStatus } from '../lib/cloudCompanionText';
import { formatWhen } from '../lib/format';
import type { CloudCompanionState, CloudConfigInput, CloudSetupCommand } from '../lib/bridge/types';
import { cloudMessages } from '../views/settings/sections/cloud.i18n';

export type CloudArea = 'status' | 'config' | 'readers';

const enabled = labFlag('cloud');
const state = ref<CloudCompanionState | null>(null);
const busy = ref<string | null>(null);
const outcome = ref<{ area: CloudArea; ok: boolean; text: string } | null>(null);

let inflight: Promise<void> | null = null;
const refresh = (): Promise<void> => {
  if (!enabled.value) return Promise.resolve();
  inflight ??= tauriApi.cloudCompanionState()
    .then((next) => { state.value = next; })
    .catch(() => { })
    .finally(() => { inflight = null; });
  return inflight;
};

watch(enabled, (on) => {
  if (on) void refresh();
  else { state.value = null; outcome.value = null; }
}, { immediate: true });

const failed = (area: CloudArea, cause: unknown) => {
  outcome.value = { area, ok: false, text: toUserMessage(cause, messagesOf(cloudMessages).failed) };
};

const act = async (area: CloudArea, name: string, run: () => Promise<CloudCompanionState>, done?: string): Promise<boolean> => {
  if (busy.value) return false;
  busy.value = name; outcome.value = null;
  try {
    state.value = await run();
    if (done) outcome.value = { area, ok: true, text: done };
    return true;
  } catch (cause) {
    failed(area, cause);
    void refresh();
    return false;
  } finally {
    busy.value = null;
  }
};

const upload = () => act('status', 'upload', () => tauriApi.cloudCompanionUpload());
const saveConfig = (input: CloudConfigInput, done: string) =>
  act('config', 'save', () => tauriApi.cloudCompanionSaveConfig(input), done);
const setup = (name: string, command: CloudSetupCommand, label: string | undefined, done: string) =>
  act('readers', name, async () => (await tauriApi.cloudCompanionSetup(command, label)).state, done);
const copyTaskCommands = (done: string, copyFailed: string) => act('status', 'tasks', async () => {
  const result = await tauriApi.cloudCompanionSetup('task-commands');
  try { await navigator.clipboard.writeText(result.output ?? ''); } catch { throw new Error(copyFailed); }
  return result.state;
}, done);
const open = (target: 'site' | 'logs' | 'folder') => {
  outcome.value = null;
  return tauriApi.cloudCompanionOpen(target).catch((cause) => failed('status', cause));
};

export const useCloudCompanion = () => {
  const t = useMessages(cloudMessages);
  const status = computed(() => cloudStatus(state.value, t.value, (at) => formatWhen(at)));
  return { enabled, state, busy, outcome, status, refresh, upload, saveConfig, setup, copyTaskCommands, open };
};
