import { reactive } from 'vue';
import { FALLBACK_APP_VERSION } from '../lib/appVersion';

export interface UpdateReceipt {
  from: string;
  version: string;
  notes: string;
}

const KEY = 'zeppbridge.update.receipt';
const VERSION_KEY = 'zeppbridge.update.lastLaunchedVersion';
const SEEN_KEY = 'zeppbridge.update.acknowledgedVersion';
export const BUNDLED_VERSION = '3.2.1';

const BUNDLED_NOTES = import.meta.glob('../../docs/releases/3.2.1/*.md', {
  query: '?raw', import: 'default',
}) as Record<string, () => Promise<string>>;

export const bundledNoteLocales = (): string[] =>
  Object.keys(BUNDLED_NOTES).map((path) => path.replace(/^.*\/([^/]+)\.md$/, '$1')).sort();

export async function loadBundledNotes(version: string, language: string): Promise<string | null> {
  if (version !== BUNDLED_VERSION) return null;
  const pick = (code: string) => BUNDLED_NOTES[`../../docs/releases/${BUNDLED_VERSION}/${code}.md`];
  const loader = pick(language) ?? pick('en');
  if (!loader) return null;
  try {
    return await loader();
  } catch {
    return null;
  }
}

export const leadsWithWhyHalfData = (version: string): boolean => version === '3.0.2';
export const updateNotice = reactive<{ receipt: UpdateReceipt | null }>({ receipt: null });

/** Run before mounting: the onboarding flag must describe the previous installation. */
export function prepareUpdateNotice(): void {
  try {
    const storage = window.localStorage;
    const previous = storage.getItem(VERSION_KEY);
    const legacyInstall = previous === null && storage.getItem('zeppbridge.tour.firstLaunch') === '1';
    // 3.0.0 predates receipts. Existing onboarding state proves prior use, but not a precise version.
    if (FALLBACK_APP_VERSION === BUNDLED_VERSION && (legacyInstall
      || (previous && /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(previous) && previous !== BUNDLED_VERSION))) {
      if (!storage.getItem(KEY) && storage.getItem(SEEN_KEY) !== BUNDLED_VERSION) rememberUpdate({ from: legacyInstall ? '' : previous!, version: BUNDLED_VERSION, notes: '' });
    }
    storage.setItem(VERSION_KEY, FALLBACK_APP_VERSION);
  } catch { /* Storage is optional; a fresh installation never gets upgrade notes. */ }
}

/** Save before the updater exits the process. A failed or cancelled update never qualifies. */
export function rememberUpdate(receipt: UpdateReceipt): void {
  try { window.localStorage.setItem(KEY, JSON.stringify(receipt)); } catch { /* Updating still works without storage. */ }
}

/** Only the installed target version may display this receipt, including when offline. */
export function restoreUpdateNotice(currentVersion: string): void {
  try {
    const receipt = JSON.parse(window.localStorage.getItem(KEY) ?? 'null') as UpdateReceipt | null;
    if (receipt && typeof receipt.from === 'string' && typeof receipt.notes === 'string'
      && receipt.version === currentVersion && receipt.from !== currentVersion
      && window.localStorage.getItem(SEEN_KEY) !== receipt.version) {
      updateNotice.receipt = receipt;
    }
  } catch { /* Missing or damaged storage must not interrupt launch. */ }
}

export function pendingUpdateNotice(): boolean {
  try { return window.localStorage.getItem(KEY) !== null; } catch { return false; }
}

export function settleUpdate(): void {
  try {
    const storage = window.localStorage;
    const version = updateNotice.receipt?.version
      ?? (JSON.parse(storage.getItem(KEY) ?? 'null') as UpdateReceipt | null)?.version;
    storage.removeItem(KEY);
    if (version) storage.setItem(SEEN_KEY, version);
  } catch { /* Close for this session. */ }
}

export function acknowledgeUpdate(): void {
  settleUpdate();
  updateNotice.receipt = null;
}

export const releaseUrl = (version: string): string =>
  `https://github.com/lingcang728/ZeppBridge/releases/tag/${encodeURIComponent(`v${version}`)}`;

export type NoteKind = 'added' | 'improved' | 'fixed' | 'note';
export interface NoteRow { text: string; kind: NoteKind; heading: boolean }

/** Release text stays text: never execute HTML or links supplied by an update manifest. */
export function releaseRows(notes: string): NoteRow[] {
  return notes.split(/\r?\n/).map(line => line.trim()).filter(Boolean).map(line => {
    const heading = /^#{1,6}\s/.test(line);
    let text = line.replace(/^#{1,6}\s+/, '').replace(/^[-*+]\s+/, '');
    // Release notes can label rows in Chinese (added, new, improved, changed, fixed) or in English.
    const match = /^(\u65b0\u589e|\u65b0\u529f\u80fd|Added|New|\u6539\u8fdb|\u6539\u9032|\u4f18\u5316|Improved|Changed|\u4fee\u590d|\u4fee\u5fa9|Fixed|Fix)\s*[:\uff1a]\s*/i.exec(text);
    let kind: NoteKind = 'note';
    if (match) {
      kind = /^(\u65b0\u589e|\u65b0\u529f\u80fd|Added|New)$/i.test(match[1]!) ? 'added'
        : /^(\u4fee\u590d|\u4fee\u5fa9|Fixed|Fix)$/i.test(match[1]!) ? 'fixed' : 'improved';
      text = text.slice(match[0].length);
    }
    return { text, kind, heading };
  });
}

export interface NoteGroup { kind: NoteKind; items: string[] }
const GROUP_ORDER: NoteKind[] = ['added', 'improved', 'fixed', 'note'];

export function groupRows(rows: NoteRow[]): NoteGroup[] {
  return GROUP_ORDER
    .map((kind) => ({ kind, items: rows.filter((row) => !row.heading && row.kind === kind).map((row) => row.text) }))
    .filter((group) => group.items.length > 0);
}

export interface NoteSegment { text: string; strong: boolean }

export function emphasis(text: string): NoteSegment[] {
  const parts = text.split(/\*\*(.+?)\*\*/);
  return parts
    .map((part, index) => ({ text: part, strong: index % 2 === 1 }))
    .filter((segment) => segment.text.length > 0);
}
