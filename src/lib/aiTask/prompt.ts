import type { AiTaskTemplate } from '../bridge/types';
import { defineMessages, messagesOf } from '../../i18n';
import { aiTaskTextFor, coverageNoteText } from './copy';

export { coverageNoteText };

const messages = defineMessages(
  { directionHeading: 'Analysis direction: ' },
);

export const templateName = (template: AiTaskTemplate): string =>
  aiTaskTextFor(template.name_code) ?? template.name;

export const templatePromptSeed = (template: AiTaskTemplate): string =>
  aiTaskTextFor(template.prompt_code) ?? template.prompt_template;

export const directionText = (template: AiTaskTemplate | null | undefined): string | null => {
  if (!template) return null;
  const seed = templatePromptSeed(template).trim();
  return seed ? `${messagesOf(messages).directionHeading}${seed}` : null;
};

export const composePromptPreview = (parts: {
  brief?: string | null;
  direction: string | null;
  question: string;
  coverageNote?: string;
}): string =>
  [(parts.brief ?? '').trim(), parts.direction ?? '', parts.question.trim(), (parts.coverageNote ?? coverageNoteText()).trim()]
    .filter((part) => part.length > 0)
    .join('\n\n');
