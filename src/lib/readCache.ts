import { dataRevision, streamRevisions, streamUpdate } from '../composables/sync/state';

export interface Query<T> {
  key: string;
  fetch: () => Promise<T>;
  deps?: readonly string[];
}

const FRESH_MS = 5 * 60_000;
const STALE_MS = 30 * 60_000;
const MAX_ENTRIES = 48;

type Entry = {
  stamp: string;
  deps?: readonly string[];
  at: number;
  promise: Promise<unknown>;
  value?: unknown;
  valueAt: number;
  valueStamp: string;
  hasValue: boolean;
  settled: boolean;
};
const entries = new Map<string, Entry>();

const touch = (key: string, entry: Entry) => {
  entries.delete(key);
  entries.set(key, entry);
};

const evict = () => {
  for (const [key, entry] of entries) {
    if (entries.size <= MAX_ENTRIES) return;
    if (entry.settled) entries.delete(key);
  }
};

const stamp = (deps?: readonly string[]) => {
  const streams = deps
    ? deps.map((stream) => streamRevisions.value[stream] ?? 0).join('.')
    : streamUpdate.value.revision;
  return `${dataRevision.value}:${streams}`;
};
const fresh = (entry: Entry | undefined): boolean =>
  Boolean(entry && entry.stamp === stamp(entry.deps) && Date.now() - entry.at < FRESH_MS);

export const cached = <T>(query: Query<T>): Promise<T> => {
  const previous = entries.get(query.key);
  if (previous && fresh(previous)) {
    touch(query.key, previous);
    return previous.promise as Promise<T>;
  }
  const entry: Entry = {
    stamp: stamp(query.deps),
    deps: query.deps,
    at: Date.now(),
    promise: Promise.resolve(),
    value: previous?.value,
    valueAt: previous?.valueAt ?? 0,
    valueStamp: previous?.valueStamp ?? '',
    hasValue: previous?.hasValue ?? false,
    settled: false,
  };
  entry.promise = query.fetch().then(
    (value) => {
      entry.value = value;
      entry.valueAt = Date.now();
      entry.valueStamp = entry.stamp;
      entry.hasValue = true;
      entry.settled = true;
      evict();
      return value;
    },
    (cause: unknown) => {
      entry.stamp = '';
      entry.settled = true;
      evict();
      throw cause;
    },
  );
  touch(query.key, entry);
  evict();
  return entry.promise as Promise<T>;
};

export const peek = <T>(query: Query<T>): { value: T; fresh: boolean } | null => {
  const hit = entries.get(query.key);
  if (!hit?.hasValue || Date.now() - hit.valueAt > STALE_MS) return null;
  touch(query.key, hit);
  return { value: hit.value as T, fresh: fresh(hit) && hit.valueStamp === hit.stamp };
};

type ValuesOf<Q extends readonly Query<unknown>[]> = { -readonly [K in keyof Q]: Q[K] extends Query<infer V> ? V : never };
export const peekAll = <Q extends readonly Query<unknown>[]>(queries: Q): ValuesOf<Q> | null => {
  const values: unknown[] = [];
  for (const query of queries) {
    const hit = peek(query);
    if (!hit) return null;
    values.push(hit.value);
  }
  return values as ValuesOf<Q>;
};

export const DEVICE_READ_PREFIXES = ['device_profile:', 'workout_device:'] as const;

export const forgetReads = (...prefixes: string[]) => {
  for (const key of [...entries.keys()]) {
    if (prefixes.some((prefix) => key.startsWith(prefix))) entries.delete(key);
  }
};

export const resetReadCache = () => entries.clear();
