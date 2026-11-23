import { computed, readonly, ref } from 'vue';
import { defineMessages, useMessages } from '../i18n';
import {
  FOCUS_AREA_KEYS, FOCUS_AREAS, FOCUS_CHOICES, areasForChoice, focusChoiceOf, normalizeAreas, type FocusArea, type FocusChoice,
} from '../lib/focusAreas';

const AREAS_KEY = 'zb.focusAreas';
const ASKED_KEY = 'zb.focusAsked';

const readAreas = (): FocusArea[] => {
  try {
    return normalizeAreas(JSON.parse(window.localStorage.getItem(AREAS_KEY) || '[]'));
  } catch {
    return [];
  }
};

const readAsked = (): boolean => {
  try {
    return window.localStorage.getItem(ASKED_KEY) === '1';
  } catch {
    return false;
  }
};

const areas = ref<FocusArea[]>(readAreas());
const asked = ref<boolean>(readAsked());

const writeAreas = (next: FocusArea[]) => {
  try {
    window.localStorage.setItem(AREAS_KEY, JSON.stringify(next));
  } catch {
  }
};

const writeAsked = () => {
  try {
    window.localStorage.setItem(ASKED_KEY, '1');
  } catch { }
};

export const setFocusAreas = (next: readonly string[]): FocusArea[] => {
  const picked = normalizeAreas(next);
  areas.value = picked;
  asked.value = true;
  writeAreas(picked);
  writeAsked();
  return picked;
};

export const skipFocusPrompt = (): void => {
  asked.value = true;
  writeAsked();
};

const areaWords = defineMessages(
  {
    areaAll: 'Balanced',
    areaSleep: 'Sleep',
    areaDaily: 'Daily status',
    areaTraining: 'Training',
  },
);
const words = useMessages(areaWords);

export const focusAreaLabel = (area: FocusArea): string =>
  (words.value as Record<string, string>)[FOCUS_AREA_KEYS[area]] ?? area;

const choice = computed(() => focusChoiceOf(areas.value));

export const useFocusAreas = () => ({
  areas: readonly(areas),
  asked: readonly(asked),
  choice,
  setAreas: setFocusAreas,
  setChoice: (next: FocusChoice) => setFocusAreas(areasForChoice(next)),
  skipPrompt: skipFocusPrompt,
});

export const focusAreaList = computed(() => FOCUS_AREAS.map((area) => ({ area, label: focusAreaLabel(area) })));

export const focusChoiceItems = computed(() => FOCUS_CHOICES.map((value) => ({
  value,
  label: value === 'all' ? words.value.areaAll : focusAreaLabel(value),
})));
