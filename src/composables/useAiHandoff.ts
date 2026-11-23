import { revealItemInDir } from '@tauri-apps/plugin-opener';
import { ref } from 'vue';
import { isTauri, tauriApi, toUserMessage } from './useTauriApi';
import type { AiHandoffResult, ExportSelection } from '../types';
import { isFixedAiProviderUrl, type AiProvider } from '../lib/aiProviders';
import { defineMessages, messagesOf } from '../i18n';
import { createLoadSeq } from '../lib/loadSeq';

const messages = defineMessages(
  {
    clipboardUnsupported: 'Clipboard write not supported here',
    targetNotAllowed: 'That AI address is not on the allow-list',
    handoffFailed: 'AI handoff failed',
    copiedButCannotOpen: (label: string) => `Copied, but could not open ${label}`,
  },
);

const copy = () => messagesOf(messages);

export type AiHandoffState = 'idle' | 'preparing' | 'opened' | 'copied_only' | 'attachment' | 'failed';


export const copyTextToClipboard = async (text: string): Promise<void> => {
  if (!navigator.clipboard?.writeText) {
    throw new Error(copy().clipboardUnsupported);
  }
  await navigator.clipboard.writeText(text);
};

export const openProviderSite = async (provider: AiProvider): Promise<'opened' | 'skipped'> => {
  if (!isFixedAiProviderUrl(provider.url)) {
    throw new Error(copy().targetNotAllowed);
  }
  if (!isTauri()) return 'skipped';
  await tauriApi.openExternalUrl(provider.url);
  return 'opened';
};

export const revealInFolder = async (path: string): Promise<void> => {
  if (!isTauri() || !path) return;
  await revealItemInDir(path);
};

export function useAiHandoff() {
  const handoffState = ref<AiHandoffState>('idle');
  const handoffError = ref<string | null>(null);
  const loadSeq = createLoadSeq();

  const copyToClipboard = copyTextToClipboard;

  const prepareAndCopy = async (
    provider: AiProvider,
    selection: ExportSelection,
    prompt: string,
    includePreciseRoute: boolean,
  ) => {
    // A second call (user picked a different provider before the first
    // one's backend round-trip returned) must not let the *older* call's
    // clipboard write / browser navigation fire after the newer one's —
    // otherwise whichever happens to resolve last wins, even if it was the
    // stale request, and the clipboard silently ends up holding the wrong
    // provider's exported health data.
    const seq = loadSeq.next();
    handoffState.value = 'preparing';
    handoffError.value = null;

    if (!isFixedAiProviderUrl(provider.url)) {
      const error = new Error(copy().targetNotAllowed);
      if (loadSeq.isCurrent(seq)) {
        handoffState.value = 'failed';
        handoffError.value = error.message;
      }
      throw error;
    }

    let result: AiHandoffResult;
    try {
      result = await tauriApi.prepareAiHandoff(selection, prompt, includePreciseRoute);
      if (!loadSeq.isCurrent(seq)) return result;
      await copyToClipboard(result.clipboardText);
    } catch (error) {
      if (loadSeq.isCurrent(seq)) {
        handoffState.value = 'failed';
        handoffError.value = toUserMessage(error, copy().handoffFailed);
      }
      throw error;
    }

    if (!isTauri()) {
      // A web preview can copy text, but it must not claim that a desktop
      // browser was opened by the Tauri opener plugin.
      if (loadSeq.isCurrent(seq)) {
        handoffState.value = result.mode === 'attachment' ? 'attachment' : 'copied_only';
      }
      return result;
    }

    if (!loadSeq.isCurrent(seq)) return result;
    try {
      await tauriApi.openExternalUrl(provider.url);
      if (loadSeq.isCurrent(seq)) {
        handoffState.value = result.mode === 'attachment' ? 'attachment' : 'opened';
      }
    } catch (error) {
      if (loadSeq.isCurrent(seq)) {
        // Clipboard succeeded; keep that fact without pretending that the
        // browser navigation succeeded.
        handoffState.value = 'copied_only';
        handoffError.value = toUserMessage(error, copy().copiedButCannotOpen(provider.label));
      }
    }
    return result;
  };

  return {
    handoffState,
    handoffError,
    prepareAndCopy,
  };
}
