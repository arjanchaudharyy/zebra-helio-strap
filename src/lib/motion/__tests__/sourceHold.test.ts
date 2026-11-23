import { describe, expect, it } from 'vitest';
import { fadeSourceIn, holdSource, isSourceHeld, sourceInFrames } from '../sourceHold';

const fakeEl = () => {
  const attrs = new Map<string, string>();
  const anims: Array<{ cancelled: boolean }> = [];
  const el = {
    isConnected: true,
    setAttribute: (k: string, v: string) => { attrs.set(k, v); },
    removeAttribute: (k: string) => { attrs.delete(k); },
    animate: () => { const a = { cancelled: false, cancel() { this.cancelled = true; } }; anims.push(a); return a; },
  };
  return { el: el as unknown as HTMLElement, attrs, anims };
};

describe('sourceHold', () => {
  it('stays hidden until the last holder releases', () => {
    const { el, attrs } = fakeEl();
    const a = holdSource(el);
    const b = holdSource(el);
    expect(attrs.get('data-source-hold')).toBe('held');
    a();
    a();
    expect(isSourceHeld(el)).toBe(true);
    b();
    expect(isSourceHeld(el)).toBe(false);
    expect(attrs.has('data-source-hold')).toBe(false);
  });

  it('the fade-back is removed on release or on a new hold', () => {
    const { el, attrs, anims } = fakeEl();
    const release = holdSource(el);
    fadeSourceIn(el, sourceInFrames(0.6, 0.9), { duration: 400 });
    expect(attrs.get('data-source-hold')).toBe('fading');
    const again = holdSource(el);
    expect(anims[0]?.cancelled).toBe(true);
    expect(attrs.get('data-source-hold')).toBe('held');
    release();
    again();
    expect(attrs.has('data-source-hold')).toBe(false);
  });

  it('an element not held does not fade; an element not in the document is not hidden', () => {
    const { el, attrs } = fakeEl();
    expect(fadeSourceIn(el, sourceInFrames(0, 1), { duration: 1 })).toBeNull();
    const detached = fakeEl();
    (detached.el as unknown as { isConnected: boolean }).isConnected = false;
    holdSource(detached.el)();
    expect(detached.attrs.size).toBe(0);
    expect(attrs.size).toBe(0);
  });
});
