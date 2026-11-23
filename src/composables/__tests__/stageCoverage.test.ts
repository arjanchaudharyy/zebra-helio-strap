import { describe, expect, it } from 'vitest';
import type { AiTask, AiTaskPreview } from '../../lib/bridge/types';
import { newTaskDraft } from '../../lib/aiTask/draft';
import { categoryRangeOf, withCategoryRange } from '../../lib/aiTask/categories';
import { freshCoverage } from '../ai/useStageCards';

const preview = (rows: Array<[string, number, number]>): AiTaskPreview => ({
  task_id: 't', workouts: [], attachments: [], estimated_bytes: 0, warnings: [],
  coverage: rows.map(([category, have, total]) => ({ category, workout_id: null, days_with_data: have, days_in_range: total } as unknown as AiTaskPreview['coverage'][number])),
});

describe('freshCoverage', () => {
  it('a preview built for the current range is used as is', () => {
    const draft = newTaskDraft();
    expect(freshCoverage(preview([['sleep', 13, 14]]), draft, draft, 'sleep')?.days_with_data).toBe(13);
  });

  it('range changed and the new preview is not back: old numbers are not shown', () => {
    const before = newTaskDraft();
    const after: AiTask = { ...before, categories: withCategoryRange(before.categories, { ...categoryRangeOf(before.categories, 'training'), picked_days: ['2026-10-08'] }) };
    const old = preview([['training', 7, 7], ['sleep', 13, 14]]);
    expect(freshCoverage(old, before, after, 'training')).toBeNull();
    expect(freshCoverage(old, before, after, 'sleep')?.days_with_data).toBe(13);
  });

  it('linked workout changed: every category waits for new results', () => {
    const before = newTaskDraft();
    const after: AiTask = { ...before, workout_ids: ['w1'] };
    expect(freshCoverage(preview([['sleep', 13, 14]]), before, after, 'sleep')).toBeNull();
  });

  it('never computed: null', () => {
    expect(freshCoverage(null, null, newTaskDraft(), 'sleep')).toBeNull();
  });
});
