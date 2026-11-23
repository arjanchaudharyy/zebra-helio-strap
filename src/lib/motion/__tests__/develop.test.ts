import { afterEach, describe, expect, it, vi } from 'vitest';
import { developWash, markDeveloped, resetDevelop, shouldDevelop } from '../develop';
import { motionTier } from '../tier';

describe('develop reveal', () => {
  afterEach(() => {
    resetDevelop();
    motionTier.value = 'high';
    vi.unstubAllGlobals();
  });

  it('one item develops only once', () => {
    motionTier.value = 'high';
    expect(shouldDevelop('sleep:1')).toBe(true);
    markDeveloped('sleep:1');
    expect(shouldDevelop('sleep:1')).toBe(false);
    expect(shouldDevelop('sleep:2')).toBe(true);
  });

  it('low-end machines do not develop', () => {
    motionTier.value = 'low';
    expect(shouldDevelop('sleep:3')).toBe(false);
  });

  it('reduced motion does not develop and removes the veil at once', () => {
    motionTier.value = 'high';
    vi.stubGlobal('document', { documentElement: { dataset: { effects: 'lite' } } });
    expect(shouldDevelop('sleep:4')).toBe(false);
    const done = vi.fn();
    const wash = { animate: vi.fn() } as unknown as HTMLElement;
    expect(developWash(wash, done)).toBeNull();
    expect(done).toHaveBeenCalledOnce();
    expect((wash as unknown as { animate: ReturnType<typeof vi.fn> }).animate).not.toHaveBeenCalled();
  });

  it('finish and cancel both remove the veil', async () => {
    motionTier.value = 'high';
    const run = (outcome: 'finish' | 'cancel') => {
      let settle!: () => void, fail!: (e: unknown) => void;
      const finished = new Promise<void>((resolve, reject) => { settle = resolve; fail = reject; });
      const wash = { animate: vi.fn(() => ({ finished })) } as unknown as HTMLElement;
      const done = vi.fn();
      developWash(wash, done, 100);
      if (outcome === 'finish') settle(); else fail(new DOMException('aborted', 'AbortError'));
      return { done, finished: finished.catch(() => undefined) };
    };
    const a = run('finish'), b = run('cancel');
    await a.finished; await b.finished; await Promise.resolve();
    expect(a.done).toHaveBeenCalledOnce();
    expect(b.done).toHaveBeenCalledOnce();
  });
});
