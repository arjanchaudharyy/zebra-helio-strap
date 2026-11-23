import { describe, expect, it } from 'vitest';
import {
  AI_TASK_PROMPT_MAX,
  applyTemplateToDraft,
  dropLegacyMetricExclusions,
  isTaskDirty,
  newTaskDraft,
  taskSnapshot,
} from '../draft';
import { defaultCategoryRanges } from '../categories';
import type { AiTaskTemplate } from '../../bridge/types';

const template = (patch: Partial<AiTaskTemplate> = {}): AiTaskTemplate => ({
  schema_version: 1,
  id: 'tpl-1',
  builtin: true,
  name: 'Template',
  name_code: null,
  categories: defaultCategoryRanges().map((range) =>
    range.category === 'sleep' ? { ...range, days_before: 30 } : range),
  detail_level: 'detailed',
  prompt_template: 'seed prompt',
  prompt_code: null,
  default_summaries: [],
  required_series: [],
  created_at: '',
  updated_at: '',
  ...patch,
});

describe('newTaskDraft', () => {
  it('privacy defaults: precise GPS off, MCP off, no attachments', () => {
    const task = newTaskDraft();
    expect(task.include_precise_gps).toBe(false);
    expect(task.mcp_shared).toBe(false);
    expect(task.attachments).toEqual([]);
    expect(task.id).toBe('');
    expect(task.schema_version).toBe(1);
  });

  it('data categories are on by default; content categories (personal_note/attachment) are off', () => {
    const task = newTaskDraft();
    const byCategory = new Map(task.categories.map((range) => [range.category, range]));
    expect(byCategory.get('sleep')?.enabled).toBe(true);
    expect(byCategory.get('personal_note')?.enabled).toBe(false);
    expect(byCategory.get('attachment')?.enabled).toBe(false);
    expect(byCategory.get('sleep')?.days_before).toBe(13);
  });
});

describe('applyTemplateToDraft', () => {
  it('a template brings a direction and suggested ranges; question, workouts and attachments stay as they are', () => {
    const task = {
      ...newTaskDraft(),
      prompt: 'my own question',
      workout_ids: ['w-1'],
      attachments: [{
        id: 'a-1', path: 'C:\\x\\report.pdf', display_name: 'report.pdf',
        kind: 'pdf' as const, byte_len: 10, added_at: '2026-01-01T00:00:00Z',
      }],
      personal_note: 'keep me',
    };
    const next = applyTemplateToDraft(task, template());
    expect(next.template_id).toBe('tpl-1');
    expect(next.detail_level).toBe('detailed');
    expect(next.prompt).toBe('my own question');
    expect(next.workout_ids).toEqual(['w-1']);
    expect(next.attachments).toHaveLength(1);
    expect(next.personal_note).toBe('keep me');
    expect(next.categories.find((range) => range.category === 'sleep')?.days_before).toBe(30);
  });

  it('content categories (attachments, personal note) follow their content; the template does not override them', () => {
    const task = {
      ...newTaskDraft(),
      categories: newTaskDraft().categories.map((range) =>
        range.category === 'attachment' ? { ...range, enabled: true } : range),
    };
    const next = applyTemplateToDraft(task, template({
      categories: defaultCategoryRanges().map((range) =>
        range.category === 'attachment' ? { ...range, enabled: false } : range),
    }));
    expect(next.categories.find((range) => range.category === 'attachment')?.enabled).toBe(true);
  });
});

describe('taskSnapshot / isTaskDirty', () => {
  it('field order and workout/attachment order do not change the snapshot', () => {
    const a = { ...newTaskDraft(), workout_ids: ['b', 'a'], title: 't' };
    const b = { ...newTaskDraft(), workout_ids: ['a', 'b'], title: 't' };
    expect(taskSnapshot(a)).toBe(taskSnapshot(b));
  });

  it('dirty flag: only a change makes it dirty', () => {
    const task = newTaskDraft();
    const baseline = taskSnapshot(task);
    expect(isTaskDirty(task, baseline)).toBe(false);
    expect(isTaskDirty({ ...task, prompt: 'x' }, baseline)).toBe(true);
    expect(isTaskDirty({ ...task, id: 'server-set', created_at: 't', updated_at: 't' }, baseline)).toBe(false);
  });
});

describe('dropLegacyMetricExclusions', () => {
  it('excluded_metrics of an old task are cleared and counted; other fields stay', () => {
    const task = newTaskDraft();
    task.categories = task.categories.map((range) =>
      range.category === 'sleep' ? { ...range, excluded_metrics: ['rem_minutes', 'deep_minutes'] } : range);
    const { task: cleaned, droppedCount } = dropLegacyMetricExclusions(task);
    expect(droppedCount).toBe(2);
    expect(cleaned.categories.find((range) => range.category === 'sleep')?.excluded_metrics).toEqual([]);
    expect(cleaned.title).toBe(task.title);
    expect(cleaned.workout_ids).toBe(task.workout_ids);
  });

  it('with no exclusions it returns the same object (droppedCount = 0)', () => {
    const task = newTaskDraft();
    const { task: cleaned, droppedCount } = dropLegacyMetricExclusions(task);
    expect(droppedCount).toBe(0);
    expect(cleaned).toBe(task);
  });
});

describe('AI_TASK_PROMPT_MAX', () => {
  it('the prompt limit is 500, as in the design', () => {
    expect(AI_TASK_PROMPT_MAX).toBe(500);
  });
});
