import { ref, type Ref } from 'vue';

export type LabFlag = 'cloud';

const KEY = 'zeppbridge.labs';

const read = (): Partial<Record<LabFlag, boolean>> => {
  try {
    const raw = window.localStorage.getItem(KEY);
    return raw ? (JSON.parse(raw) as Partial<Record<LabFlag, boolean>>) : {};
  } catch {
    return {};
  }
};

const flags: Record<LabFlag, Ref<boolean>> = {
  cloud: ref(read().cloud === true),
};

export const labFlag = (flag: LabFlag): Ref<boolean> => flags[flag];

export const setLabFlag = (flag: LabFlag, on: boolean): void => {
  flags[flag].value = on;
  try {
    window.localStorage.setItem(KEY, JSON.stringify(Object.fromEntries(Object.entries(flags).map(([k, v]) => [k, v.value]))));
  } catch {
  }
};
