import { computed, ref, watch } from 'vue';
import { AI_PROVIDERS, type AiProvider, type AiProviderId } from '../../lib/aiProviders';
import { currentProviderId, FREE_TOKEN_BUDGET, formatTokens, isSubscribed } from '../../lib/aiTask/budget';
import { handoffParts } from '../../lib/aiTask/handoffParts';
import { markdownGuide } from '../../lib/aiTask/markdownGuide';
import { planGuide } from '../../lib/aiTask/planGuide';
import { isDesktop } from '../../lib/bridge';
import type { AiTask, AiTaskPrepareResult, AiTaskPreview } from '../../lib/bridge/types';
import { useAiTaskDraft } from '../useAiTaskDraft';
import { useAiTaskHandoff } from '../useAiTaskHandoff';
import { useAiTaskPreview } from '../useAiTaskPreview';
import { useSyncController } from '../useSyncController';
import { useAiDerived } from './useAiHub';
import { useHandoffText } from '../../components/ai/HandoffDock.i18n';

export type FutureFace = 'question' | 'receipt' | 'plan';

const handoff = useAiTaskHandoff();
const provider = ref<AiProvider>(handoff.lastProvider.value ?? AI_PROVIDERS[0]!);
const face = ref<FutureFace>('question');
const recalled = ref(false);
const gathering = ref(false);
watch(provider, (next) => { currentProviderId.value = next.id; }, { immediate: true });

export interface SendChoreography {
  attach: () => Promise<void>;
  merge: (swap: () => void) => Promise<void>;
  retract: () => Promise<void>;
}

const optionsFor = (task: AiTask, preview: AiTaskPreview | null, direction: string | null, id: AiProviderId) => {
  const parts = handoffParts(task, preview, { hasDirection: Boolean(direction), now: new Date(), format: 'md' });
  return {
    briefText: parts.brief, dataFileStem: parts.dataStem, promptFileStem: parts.promptStem, promptOverride: null,
    provider: id, tokenBudget: preview?.markdown?.token_budget ?? FREE_TOKEN_BUDGET,
    markdownGuide: `${markdownGuide()}\n<!-- zeppbridge-final-plan -->\n${planGuide()}`,
  };
};

export const useStageSend = () => {
  const ctl = useAiTaskDraft();
  const derived = useAiDerived();
  const coverage = useAiTaskPreview();
  const { isSyncing } = useSyncController();
  const t = useHandoffText();
  const desktop = isDesktop();

  const ready = computed(() => (handoff.prepareResult.value?.status === 'ready' ? handoff.prepareResult.value : null));
  const blocked = computed(() => (handoff.prepareResult.value?.status === 'blocked' ? handoff.prepareResult.value.blocked : []));
  const exportTask = () => ({ ...ctl.draft.value, title: ctl.draft.value.title.trim() || derived.title.value });
  const stale = computed(() => handoff.isStale(exportTask()));
  const busy = computed(() => handoff.inFlight.value || gathering.value || handoff.steps.value.prepare.state === 'doing');
  const disabled = computed(() => !desktop || isSyncing.value);
  const subscribed = computed(() => isSubscribed(provider.value.id));

  const mdLine = computed(() => {
    const md = coverage.preview.value?.markdown;
    if (!md) return null;
    if (md.over_budget) return t.value.tooLong;
    const tokens = formatTokens(md.approx_tokens);
    if (md.approx_tokens <= FREE_TOKEN_BUDGET) return t.value.tokensFree(tokens);
    return subscribed.value ? t.value.tokensPaid(tokens) : t.value.tokensNeedPaid(tokens);
  });
  const readiness = computed(() => {
    const rows = coverage.preview.value?.coverage ?? [];
    if (!rows.length) return null;
    const categories = new Set(rows.map((row) => row.category)).size;
    const total = rows.reduce((sum, row) => sum + row.days_in_range, 0);
    const have = rows.reduce((sum, row) => sum + row.days_with_data, 0);
    return { categories, percent: total > 0 ? Math.round((have / total) * 100) : 0 };
  });
  const issueCount = computed(() => (coverage.preview.value?.warnings.length ?? 0) + (coverage.previewError.value ? 1 : 0));

  let recall: (() => void) | null = null;

  const send = async (motion: SendChoreography, onPrepared?: (result: AiTaskPrepareResult) => void) => {
    if (disabled.value || busy.value) return;
    recalled.value = false;
    gathering.value = true;
    const target = provider.value;
    const direction = derived.direction.value;
    const preview = coverage.preview.value;
    let interrupted = false;
    const recalledNow = new Promise<void>((resolve) => { recall = () => { interrupted = true; resolve(); }; });
    try {
      await handoff.start(() => ctl.saveDraft(derived.title.value), async () => {
        const task = exportTask();
        const preparing = handoff.runPrepare(task, direction, optionsFor(task, preview, direction, target.id));
        const failed = preparing.then((r) => (r?.status === 'ready' ? new Promise<void>(() => undefined) : undefined));
        await Promise.race([Promise.all([motion.attach(), preparing]), recalledNow, failed]);
        if (interrupted) {
          await motion.retract();
          await preparing;
          return;
        }
        const result = await preparing;
        if (!result || result.status !== 'ready') { await motion.retract(); return; }
        onPrepared?.(result);
        await motion.merge(() => { face.value = 'receipt'; });
        await new Promise((resolve) => window.setTimeout(resolve, 400));
        if (interrupted) { recalled.value = true; return; }
        if (await handoff.runCopy()) await handoff.runOpen(target);
      });
    } finally {
      gathering.value = false;
      recall = null;
    }
  };

  return {
    provider, face, recalled, gathering, ready, blocked, stale, busy, disabled, mdLine, readiness, issueCount, subscribed,
    steps: handoff.steps, saveError: handoff.saveError,
    send,
    exportOnly: async () => {
      if (disabled.value || busy.value) return;
      const preview = coverage.preview.value;
      const direction = derived.direction.value;
      await handoff.start(() => ctl.saveDraft(derived.title.value), async () => {
        const task = exportTask();
        await handoff.exportOnly(task, direction, optionsFor(task, preview, direction, provider.value.id));
        if (handoff.prepareResult.value?.status === 'ready') face.value = 'receipt';
      });
    },
    recall: () => { recall?.(); },
    pick: (id: AiProviderId) => { const next = AI_PROVIDERS.find((p) => p.id === id); if (next) provider.value = next; },
    copyAgain: () => handoff.runCopy(),
    openAgain: () => handoff.runOpen(provider.value),
    reveal: () => handoff.revealOutput(),
    retry: (id: 'copy' | 'open') => (id === 'copy' ? handoff.runCopy() : handoff.runOpen(provider.value)),
  };
};
