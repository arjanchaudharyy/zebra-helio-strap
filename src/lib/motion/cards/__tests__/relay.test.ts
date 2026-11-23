import { describe, expect, it } from 'vitest';
import { relay } from '../reversible';

const fake = (left: number) => {
  let resolve: () => void = () => undefined;
  const a = {
    playbackRate: 1,
    playState: 'running' as AnimationPlayState,
    pending: false,
    effect: { getComputedTiming: () => ({ endTime: 1000, localTime: 1000 - left }) },
    finished: new Promise<void>((r) => { resolve = r; }),
    updatePlaybackRate(rate: number) { this.playbackRate = rate; },
    done() { this.playState = 'finished'; resolve(); },
  };
  return a;
};

describe('interrupt relay', () => {
  it('a new command gets a new ticket; the ticket of the old run is void', () => {
    const r = relay();
    const first = r.handoff();
    expect(r.live(first)).toBe(true);
    const second = r.handoff();
    expect(r.live(first)).toBe(false);
    expect(r.live(second)).toBe(true);
  });

  it('running animations finish from now within 120ms (only faster, no jump)', () => {
    const r = relay();
    const a = fake(600);
    r.track(a as unknown as Animation);
    expect(r.moving).toBe(true);
    r.handoff();
    expect(a.playbackRate).toBeCloseTo(5, 5);
    const b = fake(60);
    r.track(b as unknown as Animation);
    r.handoff();
    expect(b.playbackRate).toBe(1);
  });

  it('a finished animation no longer counts as "moving"', async () => {
    const r = relay();
    const a = fake(300);
    r.track(a as unknown as Animation);
    a.done();
    await a.finished;
    await Promise.resolve();
    expect(r.moving).toBe(false);
  });
});
