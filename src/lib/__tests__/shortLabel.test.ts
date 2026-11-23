import { describe, expect, it } from 'vitest';
import { defineMessages } from '../../i18n';
import { ownShort } from '../shortLabel';

const bundle = defineMessages({ forever: 'Keep forever', foreverShort: 'Forever' });
const pick = (tree: { foreverShort: string }) => tree.foreverShort;

describe('ownShort', () => {
  it('uses the short name the bundle defines', () => {
    expect(ownShort(bundle, pick)).toBe('Forever');
  });

  it('returns undefined when the bundle has no short name', () => {
    const bare = defineMessages({ forever: 'Keep forever' });
    expect(ownShort(bare, (tree: { forever: string; foreverShort?: string }) => tree.foreverShort)).toBeUndefined();
  });
});
