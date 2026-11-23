import type { AiTaskCategory } from '../bridge/types';
import { defineMessages, messagesOf } from '../../i18n';
import { categoryLabel } from './categories';

export interface BriefInput {
  start: string | null;
  end: string | null;
  categories: AiTaskCategory[];
  dataFile: string;
  format?: 'json' | 'md';
  attachmentCount: number;
  hasPersonalNote: boolean;
  workoutCount: number;
  hasQuestion: boolean;
}

const messages = defineMessages(
  {
    heading: '[Task]',
    intro: (range: string) => `Attached is my personal health data exported from my Zepp watch (${range}, organised locally by Zebra; it only contains my own data).`,
    range: (start: string, end: string) => (start === end ? start : `${start} to ${end}`),
    rangeUnknown: 'a recent period',
    fileData: (file: string, labels: string) => `- ${file}: all the data. "context" lists metrics, sleep and workouts day by day per category (${labels}); "coverage" states which dates actually have data; "units" gives the unit of each field; "task.personal_note" is my own background note.`,
    fileWorkouts: (n: number) => `- "workouts" holds the ${n} workout(s) I picked on purpose — they are the focus.`,
    fileAttachments: (n: number) => `- The attachments/ folder holds ${n} original file(s) I added (e.g. lab reports, screenshots); please use them too.`,
    fileNote: '- I described my situation in the personal background; take it into account.',
    introMd: (range: string) => `This file is my personal health data exported from my Zepp watch (${range}, organised locally by Zebra; it only contains my own data).`,
    fileDataMd: (labels: string) => `- Everything after "# Data" below is the data (${labels}); the "How to read" note right after this explains the tables.`,
    fileWorkoutsMd: (n: number) => `- "Selected workouts" are the ${n} workout(s) I picked on purpose — they are the focus.`,
    fileAttachmentsMd: (n: number) => `- I also attached ${n} original file(s) (e.g. lab reports, screenshots); please use them too.`,
    start: 'Please start the analysis right away — do not ask me what I want first.',
    followQuestion: 'Answer around the "analysis direction" and my question below.',
    defaultPlan: 'I have not asked a specific question, so give me a complete analysis in this order:',
    step1: '1. One-line verdict: how I have been doing overall in this period.',
    step2: (labels: string) => `2. Category by category (${labels}): level, trend, and how it compares with my own usual baseline.`,
    step3: '3. How the categories relate — e.g. whether sleep, recovery and training load affect each other.',
    step4: '4. Anomalies worth noting: which dates or metrics clearly deviate from my norm, and likely reasons.',
    step5: '5. Actionable advice for the next 1–2 weeks: what to do, how much, and when.',
    rules: 'How to answer: conclusion first, then evidence; cite specific dates and values; point out data gaps plainly and never guess or fill in missing dates; answer in English.',
  },
);

export const buildBrief = (input: BriefInput): string => {
  const t = messagesOf(messages);
  const labels = input.categories.map(categoryLabel).join(' / ');
  const range = input.start && input.end ? t.range(input.start, input.end) : t.rangeUnknown;
  const md = input.format === 'md';
  const lines: string[] = md
    ? [t.heading, t.introMd(range), '', t.fileDataMd(labels || '—')]
    : [t.heading, t.intro(range), '', t.fileData(input.dataFile, labels || '—')];
  if (input.workoutCount > 0) lines.push(md ? t.fileWorkoutsMd(input.workoutCount) : t.fileWorkouts(input.workoutCount));
  if (input.attachmentCount > 0) lines.push(md ? t.fileAttachmentsMd(input.attachmentCount) : t.fileAttachments(input.attachmentCount));
  if (input.hasPersonalNote) lines.push(t.fileNote);
  lines.push('', t.start);
  if (input.hasQuestion) {
    lines.push(t.followQuestion);
  } else {
    lines.push(t.defaultPlan, t.step1, t.step2(labels || '—'), t.step3, t.step4, t.step5);
  }
  lines.push('', t.rules);
  return lines.join('\n');
};
