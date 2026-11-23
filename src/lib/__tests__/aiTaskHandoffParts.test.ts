import { describe, expect, it } from 'vitest';
import { exportFileStems, safeFileStem } from '../aiTask/fileName';
import { buildBrief } from '../aiTask/brief';

const now = new Date(2026, 8, 27, 16, 42);

describe('exportFileStems', () => {
  it('default rule: date range + content, prompt gets a suffix, different ranges give different names', () => {
    const a = exportFileStems('range_content', {
      start: '2026-09-14', end: '2026-09-27', categories: ['sleep', 'heart_rate', 'recovery', 'training', 'body', 'workout'], title: '', now,
    });
    expect(a.data).toBe('ZeppBridge_0914-0927_sleep and 5 more');
    expect(a.prompt).toBe('ZeppBridge_0914-0927_sleep and 5 more_prompt');
    const b = exportFileStems('range_content', { start: '2026-09-21', end: '2026-09-27', categories: ['sleep'], title: '', now });
    expect(b.data).not.toBe(a.data);
  });

  it('a range outside this year includes the year', () => {
    const s = exportFileStems('range_content', { start: '2025-12-20', end: '2026-01-02', categories: ['sleep', 'body'], title: '', now });
    expect(s.data).toBe('ZeppBridge_20251220-20260102_sleep and body');
  });

  it('the other two rules', () => {
    expect(exportFileStems('app_date', { start: null, end: null, categories: [], title: '', now }).data).toBe('ZeppBridge_2026-09-27_1642');
    expect(exportFileStems('task_time', { start: null, end: null, categories: [], title: 'Last 14 days · Sep 27', now }).data)
      .toBe('Last 14 days · Sep 27_20260927-1642');
  });

  it('replaces illegal characters and cuts long names', () => {
    expect(safeFileStem('a:b/c?d')).toBe('a_b_c_d');
    expect(Array.from(safeFileStem('a'.repeat(80))).length).toBeLessThanOrEqual(48);
  });
});

describe('buildBrief', () => {
  const base = {
    start: '2026-09-14', end: '2026-09-27', categories: ['sleep', 'heart_rate'] as const, dataFile: 'x.json',
    attachmentCount: 0, hasPersonalNote: false, workoutCount: 0,
  };

  it('no question: gives a clear task and asks to start right away', () => {
    const text = buildBrief({ ...base, categories: [...base.categories], hasQuestion: false });
    expect(text).toContain('x.json');
    expect(text).toContain('Please start the analysis right away');
    expect(text).toContain('1. One-line verdict');
    expect(text).toContain('never guess');
  });

  it('with a question: follows the question, no default list', () => {
    const text = buildBrief({ ...base, categories: [...base.categories], hasQuestion: true, attachmentCount: 2 });
    expect(text).not.toContain('1. One-line verdict');
    expect(text).toContain('2 original file(s)');
  });

  it('single .md: says the data is in this file, never names a JSON file', () => {
    const text = buildBrief({ ...base, categories: [...base.categories], hasQuestion: false, workoutCount: 1, format: 'md' });
    expect(text).toContain('# Data');
    expect(text).toContain('Selected workouts');
    expect(text).not.toContain('x.json');
  });
});
