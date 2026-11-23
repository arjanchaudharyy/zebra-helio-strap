import type { AiTaskCategory } from '../bridge/types';
import { defineMessages, messagesOf } from '../../i18n';

export type FileNameRule = 'range_content' | 'task_time' | 'app_date';
export const FILE_NAME_RULES: readonly FileNameRule[] = ['range_content', 'task_time', 'app_date'];
export const DEFAULT_FILE_NAME_RULE: FileNameRule = 'range_content';

const RULE_KEY = 'zeppbridge.ai.fileNameRule';
const MAX_STEM = 48;

const messages = defineMessages(
  {
    promptSuffix: 'prompt',
    cat_workout: 'workouts',
    cat_sleep: 'sleep',
    cat_recovery: 'recovery',
    cat_resting_hr: 'resting HR',
    cat_heart_rate: 'heart rate',
    cat_training: 'training',
    cat_body: 'body',
    cat_personal_note: 'notes',
    cat_attachment: 'files',
    twoCategories: (a: string, b: string) => `${a} and ${b}`,
    manyCategories: (a: string, _b: string, n: number) => `${a} and ${n - 1} more`,
    noData: 'no data',
  },
);

export const isFileNameRule = (value: unknown): value is FileNameRule =>
  typeof value === 'string' && (FILE_NAME_RULES as readonly string[]).includes(value);

export const readFileNameRule = (): FileNameRule => {
  try {
    const raw = window.localStorage.getItem(RULE_KEY);
    if (isFileNameRule(raw)) return raw;
  } catch {
  }
  return DEFAULT_FILE_NAME_RULE;
};

export const writeFileNameRule = (rule: FileNameRule): void => {
  try {
    window.localStorage.setItem(RULE_KEY, rule);
  } catch {
  }
};

export interface FileNameInput {
  start: string | null;
  end: string | null;
  categories: AiTaskCategory[];
  title: string;
  now: Date;
}

export interface ExportFileStems {
  data: string;
  prompt: string;
}

const pad = (n: number) => String(n).padStart(2, '0');
const compactDay = (iso: string, withYear: boolean) => {
  const [y, m, d] = iso.slice(0, 10).split('-');
  return withYear ? `${y}${m}${d}` : `${m}${d}`;
};

export const safeFileStem = (raw: string): string => {
  const cleaned = raw
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, '_')
    .replace(/\s+/g, ' ')
    .trim()
    .replace(/^[.\s]+|[.\s]+$/g, '');
  const chars = Array.from(cleaned);
  const cut = chars.length > MAX_STEM ? chars.slice(0, MAX_STEM).join('').trimEnd() : cleaned;
  return cut || 'Zebra';
};

const contentLabel = (categories: AiTaskCategory[]): string => {
  const t = messagesOf(messages);
  const labels = categories.map((category) => t[`cat_${category}`]);
  if (!labels.length) return t.noData;
  if (labels.length === 1) return labels[0]!;
  if (labels.length === 2) return t.twoCategories(labels[0]!, labels[1]!);
  return t.manyCategories(labels[0]!, labels[1]!, labels.length);
};

export const exportFileStems = (rule: FileNameRule, input: FileNameInput): ExportFileStems => {
  const { now } = input;
  const today = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  const clock = `${pad(now.getHours())}${pad(now.getMinutes())}`;
  let stem: string;
  if (rule === 'task_time') {
    stem = `${input.title.trim() || 'Zebra'}_${today.replace(/-/g, '')}-${clock}`;
  } else if (rule === 'app_date') {
    stem = `ZeppBridge_${today}_${clock}`;
  } else {
    const start = input.start ?? today;
    const end = input.end ?? today;
    const withYear = start.slice(0, 4) !== end.slice(0, 4) || end.slice(0, 4) !== String(now.getFullYear());
    const range = start === end ? compactDay(start, withYear) : `${compactDay(start, withYear)}-${compactDay(end, withYear)}`;
    stem = `ZeppBridge_${range}_${contentLabel(input.categories)}`;
  }
  const data = safeFileStem(stem);
  return { data, prompt: `${data}_${messagesOf(messages).promptSuffix}` };
};
