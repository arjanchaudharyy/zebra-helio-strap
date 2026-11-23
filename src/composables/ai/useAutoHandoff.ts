import { ref } from 'vue';
import { backend, isDesktop } from '../../lib/bridge';
import type { AiProvider } from '../../lib/aiProviders';

const KEY = 'zeppbridge.ai.autoPaste';
const readOn = () => {
  try { return window.localStorage.getItem(KEY) === '1'; } catch { return false; }
};

export const autoPasteOn = ref(readOn());
export const setAutoPaste = (on: boolean) => {
  autoPasteOn.value = on;
  try { window.localStorage.setItem(KEY, on ? '1' : '0'); } catch { }
};
export const autoSupported = () => isDesktop() && /Windows/i.test(navigator.userAgent);

export type ClipState = 'none' | 'file';
export type AutoStage = 'idle' | 'waiting' | 'file' | 'filled' | 'sent' | 'aborted' | 'timeout';
export const clip = ref<ClipState>('none');
export const autoStage = ref<AutoStage>('idle');

let ticket: number | null = null;
let listening: Promise<unknown> | null = null;
const listenOnce = () => {
  listening ??= backend.listen<{ ticket: number; stage: AutoStage }>('ai-handoff://auto', (event) => {
    if (event.ticket === ticket) autoStage.value = event.stage;
  }).catch(() => { listening = null; });
  return listening;
};

export const copyFileToClipboard = async (mdPath: string): Promise<boolean> => {
  try {
    await backend.aiHandoffCopyFile(mdPath);
    clip.value = 'file';
    return true;
  } catch {
    clip.value = 'none';
    return false;
  }
};

export const startAutoPaste = async (provider: AiProvider, mdPath: string, kickoff: string): Promise<boolean> => {
  if (!autoPasteOn.value || !autoSupported()) return false;
  await listenOnce();
  try {
    autoStage.value = 'waiting';
    ticket = await backend.aiHandoffAutopaste({ provider: provider.id, mdPath, kickoff, send: true });
    clip.value = 'file';
    return true;
  } catch {
    autoStage.value = 'idle';
    ticket = null;
    return false;
  }
};

export const resetAutoPaste = () => {
  if (autoStage.value === 'waiting') void backend.aiHandoffCancel().catch(() => undefined);
  ticket = null;
  autoStage.value = 'idle';
  clip.value = 'none';
};
