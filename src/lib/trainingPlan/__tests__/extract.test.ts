import { describe, expect, it } from 'vitest';
import { extractPlan } from '../extract';

const PLAN = { from: '2026-10-03', to: '2026-10-09', workouts: [{ date: '2026-10-03', sport: 'running', name: 'Easy run', steps: [{ kind: 'active', duration: '45min', target: 'hr 130-148' }] }] };
const json = JSON.stringify(PLAN, null, 2);

describe('extractPlan', () => {
  it('text around a json code block: takes the plan from the code block', () => {
    const reply = `Sure, here is your plan for next week:\n\n\`\`\`json\n${json}\n\`\`\`\n\nTake it slow.`;
    const result = extractPlan(reply);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.source).toBe('fence');
      expect(result.document.workouts).toHaveLength(1);
      expect(result.document.from).toBe('2026-10-03');
    }
  });

  it('a code block with no language tag works; a tagged block comes first', () => {
    const decoy = '```text\n{"hello": 1}\n```';
    const bare = `\`\`\`\n${json}\n\`\`\``;
    expect(extractPlan(`${decoy}\n${bare}`).ok).toBe(true);
    const tagged = `\`\`\`\n{"workouts": []}\n\`\`\`\n\`\`\`json\n${json}\n\`\`\``;
    const result = extractPlan(tagged);
    expect(result.ok && result.document.workouts).toHaveLength(1);
  });

  it('the whole reply is JSON', () => {
    const result = extractPlan(json);
    expect(result.ok && result.source).toBe('whole');
  });

  it('JSON inside text with no code block: found by bracket matching; brackets inside strings do not count', () => {
    const tricky = { workouts: [{ date: '2026-10-03', sport: 'running', name: 'name with } and {', steps: [] }] };
    const reply = `Plan below ${JSON.stringify(tricky)} good luck {not JSON}`;
    const result = extractPlan(reply);
    expect(result.ok && result.source).toBe('braces');
    expect(result.ok && result.document.workouts[0].name).toBe('name with } and {');
  });

  it('a top-level workout array is wrapped as workouts', () => {
    const result = extractPlan(JSON.stringify(PLAN.workouts));
    expect(result.ok && result.document.workouts).toHaveLength(1);
  });

  it('JSON that is not a plan gives not_a_plan, not no_json', () => {
    expect(extractPlan('```json\n{"hello": "world"}\n```')).toEqual({ ok: false, failure: 'not_a_plan' });
    expect(extractPlan('[1, 2, 3]')).toEqual({ ok: false, failure: 'not_a_plan' });
  });

  it('no JSON at all, or empty', () => {
    expect(extractPlan('Rest on Wednesday, intervals on Thursday.')).toEqual({ ok: false, failure: 'no_json' });
    expect(extractPlan('   \n ')).toEqual({ ok: false, failure: 'empty' });
  });

  it('broken JSON with unmatched brackets does not crash and is not a plan', () => {
    expect(extractPlan('{"workouts": [ {"date": "2026-10-03"')).toEqual({ ok: false, failure: 'no_json' });
  });

  it('clearing days (empty workouts + from/to) is a valid plan', () => {
    const result = extractPlan('{"from":"2026-10-03","to":"2026-10-05","workouts":[]}');
    expect(result.ok && result.document.workouts).toEqual([]);
  });
});


it('prefers the final plan/2 block over an earlier draft, while keeping source provenance', () => {
 const final = { ...PLAN, format: 'zeppbridge-plan/2', summary: 'Final agreement' };
 const reply = '```json\n'+json+'\n```\nFinal\n```json\n'+JSON.stringify(final)+'\n```';
 expect(extractPlan(reply)).toEqual({ok:true,document:final,source:'fence'});
 expect(extractPlan(JSON.stringify(final))).toEqual({ok:true,document:final,source:'whole'});
 expect(extractPlan('Final: '+JSON.stringify(final))).toEqual({ok:true,document:final,source:'braces'});
});

it('a final plan/3 (with focus, variant, description) wins; an old plan/2 still reads', () => {
  const v3 = { format: 'zeppbridge-plan/3', from: '2026-10-03', to: '2026-10-09', workouts: [{
    date: '2026-10-04', sport: 'cycling', variant: 'outdoor', name: 'Long ride', focus: 'Build endurance',
    description: 'Cadence 85–95', steps: [{ kind: 'active', duration: '90min', target: 'hr 120-140' }],
  }] };
  const old = { ...PLAN, format: 'zeppbridge-plan/2' };
  const reply = ['```json', JSON.stringify(old), '```', 'Final:', '```json', JSON.stringify(v3), '```'].join('\n');
  const result = extractPlan(reply);
  expect(result.ok && result.document).toEqual(v3);
  expect(extractPlan(JSON.stringify(old))).toEqual({ ok: true, document: old, source: 'whole' });
  expect(result.ok && result.document.workouts[0].focus).toBe('Build endurance');
});
