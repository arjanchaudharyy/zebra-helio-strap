import { ref, toRaw, watch, type Ref } from 'vue';
import { backend, isDesktop, toUserMessage } from '../lib/bridge';
import type { AiTask, AiTaskPreview } from '../lib/bridge/types';
import { taskSnapshot } from '../lib/aiTask/draft';
import { tokenBudget } from '../lib/aiTask/budget';
import { defineMessages, messagesOf } from '../i18n';

const messages = defineMessages(
  { previewFailed: 'Could not build the preview' },
);

const DEBOUNCE_MS = 600;

const preview = ref<AiTaskPreview | null>(null);
const previewTask = ref<AiTask | null>(null);
const previewLoading = ref(false);
const previewError = ref<string | null>(null);
let seq = 0;
let watching = false;

const loadPreview = async (task: AiTask) => {
  const mine = ++seq;
  previewLoading.value = true;
  previewError.value = null;
  try {
    const result = await backend.aiTaskPreview(task, tokenBudget.value);
    if (mine === seq) { preview.value = result; previewTask.value = JSON.parse(JSON.stringify(toRaw(task))) as AiTask; }
  } catch (error) {
    if (mine === seq) previewError.value = toUserMessage(error, messagesOf(messages).previewFailed);
  } finally {
    if (mine === seq) previewLoading.value = false;
  }
};

const watchDraft = (draft: Ref<AiTask>) => {
  if (!isDesktop() || watching) return;
  watching = true;
  let timer: ReturnType<typeof setTimeout> | undefined;
  watch(
    () => `${taskSnapshot(draft.value)}|${tokenBudget.value}`,
    () => {
      clearTimeout(timer);
      timer = setTimeout(() => void loadPreview(draft.value), DEBOUNCE_MS);
    },
  );
  void loadPreview(draft.value);
};

export function useAiTaskPreview() {
  return { preview, previewTask, previewLoading, previewError, loadPreview, watchDraft };
}
