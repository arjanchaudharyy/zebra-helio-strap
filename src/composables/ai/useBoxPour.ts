import { onBeforeUnmount, onMounted, watch } from 'vue';
import { AI_TASK_CATEGORY_META } from '../../lib/aiTask/categories';
import { dayKey } from '../../lib/aiTask/bridgeScale';
import { pourFromBox } from '../../lib/motion/cards';
import type { AiTaskCategory } from '../../lib/bridge/types';
import { useAiTaskDraft } from '../useAiTaskDraft';
import { useCardCollection } from '../useCardCollection';
import { paced } from '../../lib/motion/timing';

const ARRIVE_MS = paced(700);
const SETTLE_MS = paced(300);

export const useBoxPour = (options: {
  cardOf: (category: AiTaskCategory) => HTMLElement | null;
  fallback: () => HTMLElement | null;
  busy: () => boolean;
}) => {
  const box = useCardCollection();
  const ctl = useAiTaskDraft();
  let running = false;
  let timer = 0;

  const pour = async () => {
    if (running || !box.count.value || box.tableOpen.value || options.busy()) return;
    const source = document.getElementById('card-collection-box');
    if (source?.classList.contains('open')) return;
    running = true;
    try {
      const byCategory = box.byCategory();
      const workoutIds = box.workoutIds();
      const counts = new Map<AiTaskCategory, number>([...byCategory].map(([category, days]) => [category, days.length]));
      if (workoutIds.length) counts.set('workout', (counts.get('workout') ?? 0) + workoutIds.length);
      const targets = [...counts].flatMap(([category, count]) => {
        const el = options.cardOf(category) ?? options.fallback();
        return el ? [{ el, tint: AI_TASK_CATEGORY_META[category].tint, count }] : [];
      });
      await pourFromBox(source, targets);
      ctl.mergePicks(byCategory, dayKey(), workoutIds);
      box.handedToTask.value = false;
      box.clear();
    } finally {
      running = false;
    }
  };
  const later = (ms: number) => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => { void pour(); }, ms);
  };

  watch([box.count, box.tableOpen, box.pourRequest], () => later(SETTLE_MS));
  onMounted(() => later(ARRIVE_MS));
  onBeforeUnmount(() => window.clearTimeout(timer));
  return { pour };
};
