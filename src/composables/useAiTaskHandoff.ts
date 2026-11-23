import { ref } from 'vue';
import { backend, toUserMessage } from '../lib/bridge';
import type { AiTask, AiTaskPrepareOptions, AiTaskPrepareResult } from '../lib/bridge/types';
import { taskSnapshot } from '../lib/aiTask/draft';
import { coverageNoteText } from '../lib/aiTask/copy';
import { copyTextToClipboard, openProviderSite, revealInFolder } from './useAiHandoff';
import { AI_PROVIDER_BY_ID, type AiProvider, type AiProviderId } from '../lib/aiProviders';
import { defineMessages, messagesOf } from '../i18n';
import { useCardCollection } from './useCardCollection';
import { copyFileToClipboard, resetAutoPaste, startAutoPaste } from './ai/useAutoHandoff';

export type HandoffStepId = 'prepare' | 'copy' | 'open';
export type HandoffStepState = 'idle' | 'doing' | 'done' | 'failed' | 'blocked' | 'skipped';

export interface HandoffStep {
  state: HandoffStepState;
  errorText: string | null;
}

const messages = defineMessages(
  {
    prepareFailed: 'Could not prepare files', copyFailed: 'Could not copy the opening message', openFailed: 'Could not open the AI site',
    saveFailed: 'The task was not saved: the file is still prepared, but these choices will not be remembered next time and MCP task access stays as before.',
    kickoff: 'Please read the attached file. Discuss it with me first; when I say “finalize”, return a JSON plan in the format at the end of the file.',
  },
);
const copy = () => messagesOf(messages);

const idle = (): HandoffStep => ({ state: 'idle', errorText: null });
const freshSteps = (): Record<HandoffStepId, HandoffStep> => ({ prepare: idle(), copy: idle(), open: idle() });

const LAST_PROVIDER_KEY = 'zeppbridge.ai.handoff.provider';

const readLastProvider = (): AiProvider | null => {
  try {
    const raw = window.localStorage.getItem(LAST_PROVIDER_KEY);
    return raw ? AI_PROVIDER_BY_ID[raw as AiProviderId] ?? null : null;
  } catch {
    return null;
  }
};

const storeLastProvider = (provider: AiProvider) => {
  try {
    window.localStorage.setItem(LAST_PROVIDER_KEY, provider.id);
  } catch {
  }
};

export function useAiTaskHandoff() {
  const prepareResult = ref<AiTaskPrepareResult | null>(null);
  const preparedSnapshot = ref<string | null>(null);
  const steps = ref(freshSteps());
  const lastProvider = ref<AiProvider | null>(readLastProvider());
  const inFlight = ref(false);
  const saveError = ref<string | null>(null);

  const setStep = (id: HandoffStepId, state: HandoffStepState, errorText: string | null = null) => {
    steps.value = { ...steps.value, [id]: { state, errorText } };
  };

  const isStale = (task: AiTask): boolean =>
    preparedSnapshot.value !== null && taskSnapshot(task) !== preparedSnapshot.value;

  const runPrepare = async (task: AiTask, direction: string | null = null, options?: AiTaskPrepareOptions) => {
    steps.value = { ...freshSteps(), prepare: { state: 'doing', errorText: null } };
    resetAutoPaste();
    try {
      const result = await backend.aiTaskPrepare(task, coverageNoteText(), direction, options);
      prepareResult.value = result;
      preparedSnapshot.value = taskSnapshot(task);
      setStep('prepare', result.status === 'blocked' ? 'blocked' : 'done');
      if (result.status === 'ready' && task.categories.some((range) => range.picked_days?.length)) useCardCollection().clearAfterHandoff();
      return result;
    } catch (error) {
      setStep('prepare', 'failed', toUserMessage(error, copy().prepareFailed));
      return null;
    }
  };

  const runCopy = async (): Promise<boolean> => {
    const result = prepareResult.value;
    if (!result || result.status !== 'ready') return false;
    setStep('copy', 'doing');
    try {
      if (result.md_path && await copyFileToClipboard(result.md_path)) { setStep('copy', 'done'); return true; }
      await copyTextToClipboard(result.md_path ? copy().kickoff : result.prompt_text);
      setStep('copy', 'done');
      return true;
    } catch (error) {
      setStep('copy', 'failed', toUserMessage(error, copy().copyFailed));
      return false;
    }
  };

  const revealOutput = async () => {
    const result = prepareResult.value?.status === 'ready' ? prepareResult.value : null;
    await revealInFolder(result?.md_path || result?.output_dir || '').catch(() => undefined);
  };

  const runOpen = async (provider: AiProvider): Promise<boolean> => {
    lastProvider.value = provider;
    storeLastProvider(provider);
    setStep('open', 'doing');
    try {
      const md = prepareResult.value?.status === 'ready' ? prepareResult.value.md_path : null;
      if (md) await startAutoPaste(provider, md, copy().kickoff);
      const outcome = await openProviderSite(provider);
      setStep('open', outcome === 'opened' ? 'done' : 'skipped');
      return true;
    } catch (error) {
      setStep('open', 'failed', toUserMessage(error, copy().openFailed));
      return false;
    }
  };

  const runAll = async (task: AiTask, provider: AiProvider, direction: string | null = null, options?: AiTaskPrepareOptions) => {
    const prepared = await runPrepare(task, direction, options);
    if (!prepared || prepared.status !== 'ready') return;
    if (!(await runCopy())) return;
    await runOpen(provider);
    if (!prepared.md_path) await revealOutput();
  };

  const exportOnly = async (task: AiTask, direction: string | null = null, options?: AiTaskPrepareOptions) => {
    const prepared = await runPrepare(task, direction, options);
    if (prepared?.status === 'ready') await revealOutput();
  };

  const start = async (save: () => Promise<unknown>, go: () => Promise<void>): Promise<boolean> => {
    if (inFlight.value) return false;
    inFlight.value = true;
    saveError.value = null;
    try {
      try {
        await save();
      } catch {
        saveError.value = copy().saveFailed;
      }
      await go();
      return true;
    } finally {
      inFlight.value = false;
    }
  };

  return {
    inFlight,
    saveError,
    start,
    prepareResult,
    preparedSnapshot,
    steps,
    lastProvider,
    isStale,
    runPrepare,
    runCopy,
    runOpen,
    runAll,
    exportOnly,
    revealOutput,
  };
}
