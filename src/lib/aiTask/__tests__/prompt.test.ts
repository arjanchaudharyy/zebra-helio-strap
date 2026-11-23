import { describe, expect, it } from 'vitest';
import { composePromptPreview, directionText, templateName, templatePromptSeed } from '../prompt';
import { aiTaskIssueText, aiTaskTextFor, coverageNoteText } from '../copy';
import type { AiTaskTemplate } from '../../bridge/types';

const template = (patch: Partial<AiTaskTemplate> = {}): AiTaskTemplate => ({
  schema_version: 1,
  id: 't',
  builtin: false,
  name: 'Stored name',
  name_code: null,
  categories: [],
  detail_level: 'standard',
  prompt_template: 'stored prompt',
  prompt_code: null,
  default_summaries: [],
  required_series: [],
  created_at: '',
  updated_at: '',
  ...patch,
});

describe('templateName / templatePromptSeed', () => {
  it('no code uses the stored text; a code picks the UI copy', () => {
    expect(templateName(template())).toBe('Stored name');
    expect(templatePromptSeed(template())).toBe('stored prompt');
    const builtin = template({
      builtin: true,
      name_code: 'ui.ai_template.recovery_run.name',
      prompt_code: 'ui.ai_template.recovery_run.prompt',
    });
    expect(templateName(builtin)).toBe('Recovery run');
    expect(templatePromptSeed(builtin)).toContain('recovery-phase workout');
  });
});

describe('directionText / composePromptPreview', () => {
  it('no template gives no direction; a template gives a titled direction with the template text', () => {
    expect(directionText(null)).toBeNull();
    expect(directionText(template({ prompt_template: '  ' }))).toBeNull();
    const text = directionText(template());
    expect(text).toContain('stored prompt');
    expect(text).not.toBe('stored prompt');
  });

  it('direction and question coexist in order direction, question, coverage note, with no blank sections', () => {
    const composed = composePromptPreview({ direction: 'D', question: '  Q  ', coverageNote: 'C' });
    expect(composed).toBe('D\n\nQ\n\nC');
    expect(composePromptPreview({ direction: null, question: '', coverageNote: 'C' })).toBe('C');
  });
});

describe('copy table', () => {
  it('category, template and blocked codes have copy; an unknown code gives undefined', () => {
    expect(aiTaskTextFor('ui.ai_task.cat.sleep')).toBe('Sleep');
    expect(aiTaskTextFor('ui.ai_task.blocked.attachment_missing')).toBeTruthy();
    expect(aiTaskTextFor('ui.ai_task.nope')).toBeUndefined();
    expect(aiTaskTextFor(null)).toBeUndefined();
  });

  it('coverage_note is the opening line passed to ai_task_prepare', () => {
    expect(coverageNoteText()).toBe(aiTaskTextFor('ui.ai_task.prompt.coverage_note'));
  });
});

describe('aiTaskIssueText', () => {
  it('backend err.ai_task.attachment_missing uses the ui.ai_task.blocked.attachment_missing copy', () => {
    const text = aiTaskIssueText({
      code: 'err.ai_task.attachment_missing',
      message: '\u9644\u4ef6\u6587\u4ef6\u5df2\u4e0d\u5728\u539f\u4f4d\u7f6e',
    });
    expect(text).toBe(aiTaskTextFor('ui.ai_task.blocked.attachment_missing'));
  });

  it('ui.* codes get copy; an unknown code falls back to the backend text', () => {
    expect(
      aiTaskIssueText({ code: 'ui.ai_task.blocked.no_workouts', message: 'x' }),
    ).toBe(aiTaskTextFor('ui.ai_task.blocked.no_workouts'));
    expect(aiTaskIssueText({ code: 'ui.ai_task.nope', message: 'Backend text' })).toBe('Backend text');
    expect(aiTaskIssueText({ code: 'ui.ai_task.nope', message: '\u540e\u7aef\u539f\u6587' }, 'Fallback')).toBe('Fallback');
  });
});
