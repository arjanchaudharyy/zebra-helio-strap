import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  BUNDLED_VERSION, acknowledgeUpdate, emphasis, groupRows, leadsWithWhyHalfData, loadBundledNotes,
  prepareUpdateNotice, releaseRows, rememberUpdate, restoreUpdateNotice, settleUpdate, updateNotice,
} from '../updateNotice';
import { FALLBACK_APP_VERSION } from '../../lib/appVersion';

describe('release notes after a successful restart', () => {
  let storage: Map<string, string>;
  const receipt = { from: '3.0.0', version: '3.0.1', notes: 'Added: A new setting' };
  beforeEach(() => {
    storage = new Map();
    updateNotice.receipt = null;
    vi.stubGlobal('window', { localStorage: {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
      removeItem: (key: string) => storage.delete(key),
    } });
  });
  afterEach(() => vi.unstubAllGlobals());
  it('does not show before installation, after cancellation, or on a different installed version', () => {
    rememberUpdate(receipt);
    expect(updateNotice.receipt).toBeNull();
    for (const version of ['3.0.0', '2.4.4', '3.0.2']) {
      restoreUpdateNotice(version);
      expect(updateNotice.receipt).toBeNull();
    }
  });
  it('survives restart offline and reappears until acknowledged, then stays closed', () => {
    rememberUpdate(receipt);
    restoreUpdateNotice('3.0.1');
    expect(updateNotice.receipt).toEqual(receipt);
    updateNotice.receipt = null;
    restoreUpdateNotice('3.0.1');
    expect(updateNotice.receipt).toEqual(receipt);
    acknowledgeUpdate();
    restoreUpdateNotice('3.0.1');
    expect(updateNotice.receipt).toBeNull();
  });
  it('stays closed once Got it was clicked even if the close animation never finished (window sent to tray)', () => {
    rememberUpdate(receipt);
    restoreUpdateNotice('3.0.1');
    settleUpdate(); // click: the animation would run next, but the app quits from the tray before it ends
    updateNotice.receipt = null; // next launch starts with nothing in memory
    restoreUpdateNotice('3.0.1');
    expect(updateNotice.receipt).toBeNull();
  });
  it('an acknowledged version is never offered again, even if a stale receipt or launch record comes back', () => {
    rememberUpdate(receipt);
    restoreUpdateNotice('3.0.1');
    acknowledgeUpdate();
    rememberUpdate(receipt);
    restoreUpdateNotice('3.0.1');
    expect(updateNotice.receipt).toBeNull();
    // a later version still shows
    rememberUpdate({ from: '3.0.1', version: '3.0.2', notes: '' });
    restoreUpdateNotice('3.0.2');
    expect(updateNotice.receipt?.version).toBe('3.0.2');
  });
  it('does not introduce an update dialog on a fresh installation or a same-version rebuild', () => {
    restoreUpdateNotice('3.0.1');
    expect(updateNotice.receipt).toBeNull();
    rememberUpdate({ ...receipt, from: '3.0.1' });
    restoreUpdateNotice('3.0.1');
    expect(updateNotice.receipt).toBeNull();
  });
  it('tolerates blocked or corrupt storage', () => {
    storage.set('zeppbridge.update.receipt', '{');
    expect(() => restoreUpdateNotice('3.0.1')).not.toThrow();
    vi.stubGlobal('window', { get localStorage() { throw new Error('blocked'); } });
    expect(() => rememberUpdate(receipt)).not.toThrow();
    expect(() => restoreUpdateNotice('3.0.1')).not.toThrow();
    expect(() => acknowledgeUpdate()).not.toThrow();
  });
  it('shows bundled notes to a legacy user only after launching the installed new version', async () => {
    storage.set('zeppbridge.tour.firstLaunch', '1');
    prepareUpdateNotice();
    expect(updateNotice.receipt).toBeNull();
    restoreUpdateNotice('3.0.0');
    expect(updateNotice.receipt).toBeNull();
    restoreUpdateNotice(BUNDLED_VERSION);
    expect(updateNotice.receipt?.from).toBe('');
    acknowledgeUpdate();
    prepareUpdateNotice();
    restoreUpdateNotice(BUNDLED_VERSION);
    expect(updateNotice.receipt).toBeNull();
  });
  it('never mistakes first-launch onboarding or a same-version relaunch for an upgrade', () => {
    prepareUpdateNotice();
    storage.set('zeppbridge.tour.firstLaunch', '1');
    prepareUpdateNotice();
    restoreUpdateNotice('3.0.1');
    expect(updateNotice.receipt).toBeNull();
  });
  it('preserves updater receipts', () => {
    storage.set('zeppbridge.tour.firstLaunch', '1');
    rememberUpdate(receipt);
    prepareUpdateNotice();
    restoreUpdateNotice('3.0.1');
    expect(updateNotice.receipt).toEqual(receipt);
  });
  it('classifies source notes but preserves untrusted content as plain text', () => {
    expect(releaseRows('# Features\n- Added: setting\n\u4fee\u590d\uff1a\u9519\u8bef\n<script>bad()</script>')).toEqual([
      { text: 'Features', kind: 'note', heading: true },
      { text: 'setting', kind: 'added', heading: false },
      { text: '\u9519\u8bef', kind: 'fixed', heading: false },
      { text: '<script>bad()</script>', kind: 'note', heading: false },
    ]);
  });
});

describe('bundled release notes for the current version', () => {
  it('ships the notes for the version this build is', () => {
    expect(BUNDLED_VERSION).toBe(FALLBACK_APP_VERSION);
  });
  it('falls back to English for an unknown language and to nothing for another version', async () => {
    expect(await loadBundledNotes(BUNDLED_VERSION, 'xx')).toBe(await loadBundledNotes(BUNDLED_VERSION, 'en'));
    expect(await loadBundledNotes('2.9.9', 'en')).toBeNull();
  });
  it('3.0.2 opens with the missing-data explanation', () => {
    expect(leadsWithWhyHalfData('3.0.2')).toBe(true);
    expect(leadsWithWhyHalfData('3.0.1')).toBe(false);
  });
});

describe('release note layout', () => {
  it('groups rows as added, improved, fixed, then anything else, keeping authored order', () => {
    const rows = releaseRows('- Fixed: a\n- Added: b\n## Section\n- Improved: c\n- Added: d\nplain');
    expect(groupRows(rows)).toEqual([
      { kind: 'added', items: ['b', 'd'] },
      { kind: 'improved', items: ['c'] },
      { kind: 'fixed', items: ['a'] },
      { kind: 'note', items: ['plain'] },
    ]);
  });
  it('only understands **bold**, and never turns text into markup', () => {
    expect(emphasis('**Lead** then <b>x</b>')).toEqual([
      { text: 'Lead', strong: true },
      { text: ' then <b>x</b>', strong: false },
    ]);
    expect(emphasis('a ** b')).toEqual([{ text: 'a ** b', strong: false }]);
  });
});
