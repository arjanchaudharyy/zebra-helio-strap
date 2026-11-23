
export type FocusArea = 'sleep' | 'daily' | 'training';

export type FocusModule = 'heart' | 'steps' | 'sleep' | 'body' | 'training';

export const FOCUS_AREAS: readonly FocusArea[] = ['sleep', 'daily', 'training'];

export const MODULE_AREA: Record<FocusModule, FocusArea> = {
  heart: 'daily',
  steps: 'daily',
  sleep: 'sleep',
  body: 'daily',
  training: 'training',
};

const MODULE_ORDER: readonly FocusModule[] = ['heart', 'steps', 'sleep', 'body', 'training'];

const isArea = (value: unknown): value is FocusArea =>
  (FOCUS_AREAS as readonly string[]).includes(value as FocusArea);

export const normalizeAreas = (value: unknown): FocusArea[] => {
  if (!Array.isArray(value)) return [];
  const out: FocusArea[] = [];
  for (const item of value) if (isArea(item) && !out.includes(item)) out.push(item);
  return out;
};

export interface FocusSplit {
  matched: FocusModule[];
  folded: FocusModule[];
}

export const focusSplit = (areas: readonly string[]): FocusSplit | null => {
  const picked = normalizeAreas(areas);
  if (picked.length === 0 || picked.length === FOCUS_AREAS.length) return null;
  const matched = MODULE_ORDER.filter((module) => picked.includes(MODULE_AREA[module]));
  const folded = MODULE_ORDER.filter((module) => !picked.includes(MODULE_AREA[module]));
  return { matched, folded };
};

export type FocusChoice = 'all' | FocusArea;
export const FOCUS_CHOICES: readonly FocusChoice[] = ['all', ...FOCUS_AREAS];

export const focusChoiceOf = (areas: readonly string[]): FocusChoice => {
  const picked = normalizeAreas(areas);
  return picked.length === 1 ? picked[0]! : 'all';
};

export const areasForChoice = (choice: FocusChoice): FocusArea[] => (choice === 'all' ? [] : [choice]);

export const FOCUS_AREA_KEYS: Record<FocusArea, string> = {
  sleep: 'areaSleep',
  daily: 'areaDaily',
  training: 'areaTraining',
};
