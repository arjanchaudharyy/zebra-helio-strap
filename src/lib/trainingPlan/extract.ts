import type { PlanDocument, PlanWorkoutInput } from '../../types/trainingPlan';

export const FINAL_FORMATS = ['zeppbridge-plan/3', 'zeppbridge-plan/2', 'zeppbridge.plan/2'];

export type ExtractFailure = 'empty' | 'no_json' | 'not_a_plan';

export type ExtractResult =
  | { ok: true; document: PlanDocument; source: 'fence' | 'whole' | 'braces' }
  | { ok: false; failure: ExtractFailure };

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const asPlan = (value: unknown): PlanDocument | null => {
  if (Array.isArray(value)) {
    return value.every(isRecord) ? { workouts: value as unknown as PlanWorkoutInput[] } : null;
  }
  if (isRecord(value) && Array.isArray(value.workouts)) return value as unknown as PlanDocument;
  return null;
};

const tryParse = (text: string): unknown => {
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
};

const fencedBlocks = (text: string): string[] => {
  const tagged: string[] = [];
  const bare: string[] = [];
  const pattern = /```([A-Za-z0-9_-]*)[^\n]*\n([\s\S]*?)```/g;
  for (let match = pattern.exec(text); match; match = pattern.exec(text)) {
    (match[1] ? tagged : bare).push(match[2]);
  }
  return [...tagged, ...bare];
};

const bracketed = (text: string): string[] => {
  const out: string[] = [];
  for (let start = 0; start < text.length; start += 1) {
    const open = text[start];
    if (open !== '{' && open !== '[') continue;
    const close = open === '{' ? '}' : ']';
    let depth = 0;
    let inString = false;
    let end = -1;
    for (let index = start; index < text.length; index += 1) {
      const char = text[index];
      if (inString) {
        if (char === '\\') index += 1;
        else if (char === '"') inString = false;
        continue;
      }
      if (char === '"') inString = true;
      else if (char === open) depth += 1;
      else if (char === close) {
        depth -= 1;
        if (depth === 0) { end = index; break; }
      }
    }
    if (end < 0) continue;
    out.push(text.slice(start, end + 1));
    start = end;
  }
  return out;
};

export const extractPlan = (reply: string): ExtractResult => {
  const text = reply.trim();
  if (!text) return { ok: false, failure: 'empty' };

  let sawJson = false;
  const attempt = (candidate: string, source: 'fence' | 'whole' | 'braces'): ExtractResult | null => {
    const value = tryParse(candidate.trim());
    if (value === undefined) return null;
    sawJson = true;
    const document = asPlan(value);
    return document ? { ok: true, document, source } : null;
  };

  const candidates = [
    ...fencedBlocks(text).map(text => ({ text, source: 'fence' as const })),
    { text, source: 'whole' as const },
    ...bracketed(text).map(text => ({ text, source: 'braces' as const })),
  ];
  let final: { value: unknown; source: 'fence' | 'whole' | 'braces'; rank: number } | null = null;
  for (const candidate of candidates) {
    const value = tryParse(candidate.text.trim());
    const rank = isRecord(value) && asPlan(value) ? FINAL_FORMATS.indexOf(String(value.format)) : -1;
    if (rank < 0) continue;
    if (!final || rank < final.rank || (rank === final.rank && candidate.source === final.source)) {
      final = { value, source: candidate.source, rank };
    }
  }
  if (final) return { ok: true, document: asPlan(final.value)!, source: final.source };
  for (const block of fencedBlocks(text)) {
    const hit = attempt(block, 'fence');
    if (hit) return hit;
  }
  const whole = attempt(text, 'whole');
  if (whole) return whole;
  for (const piece of bracketed(text)) {
    const hit = attempt(piece, 'braces');
    if (hit) return hit;
  }
  return { ok: false, failure: sawJson ? 'not_a_plan' : 'no_json' };
};
