import { defineMessages, messagesOf } from '../../i18n';


const messages = defineMessages(
  {
    weightWhat: 'Body weight: one reading per weigh-in.',
    weightChart: 'Several weigh-ins in a day are averaged per day; the shaded band is that day’s measured range.',
    weightHow: 'Weigh-ins come from the Zepp cloud — both the official authorization and advanced data sync them; watch-recorded weight is included, but it carries no body composition.',
    bmiWhat: 'BMI: weight relative to height (kg/m²).',
    bmiChart: 'Updates with each weigh-in.',
    bmiHow: 'Sent by the cloud alongside the weight record (both the official authorization and advanced data) — not computed here from height and weight.',
    body_fat_rateWhat: 'Body fat: fat as a share of weight, in %.',
    body_fat_rateChart: 'Updates with each body-composition weigh-in; several in a day are averaged per day.',
    body_fat_rateHow: 'Needs a body-composition scale — watch and hand-entered weights carry none of this. Synced via advanced data; the official body-composition API is not open yet.',
    muscle_massWhat: 'Muscle mass, shown in your chosen unit system.',
    muscle_massChart: 'Updates with each body-composition weigh-in.',
    muscle_massHow: 'Needs a body-composition scale, synced via advanced data; the official body-composition API is not open yet.',
    body_water_rateWhat: 'Body water: water as a share of weight, in %.',
    body_water_rateChart: 'Updates with each body-composition weigh-in.',
    body_water_rateHow: 'Needs a body-composition scale, synced via advanced data; the official body-composition API is not open yet.',
    bone_massWhat: 'Bone mass, shown in your chosen unit system.',
    bone_massChart: 'Updates with each body-composition weigh-in.',
    bone_massHow: 'Needs a body-composition scale, synced via advanced data; the official body-composition API is not open yet.',
    visceral_fatWhat: 'Visceral fat: a grade of the fat around the internal organs, not a percentage.',
    visceral_fatChart: 'Updates with each body-composition weigh-in.',
    visceral_fatHow: 'Zepp scores it as a 1–30 grade. Needs a body-composition scale, synced via advanced data.',
    bmrWhat: 'Basal metabolism: the daily burn at complete rest (kcal/day).',
    bmrChart: 'Updates with each body-composition weigh-in.',
    bmrHow: 'Needs a body-composition scale, synced via advanced data; the official body-composition API is not open yet.',
    heightWhat: 'Height: a profile value, not a measurement of the day.',
    heightChart: 'Echoed back with every weigh-in; rarely changes.',
    heightHow: 'Profile data from the cloud, sent along with each weigh-in (both the official authorization and advanced data).',
    intake_caloriesWhat: 'Calories eaten: the total logged for the day.',
    intake_caloriesChart: 'Drawn as one bar per day; days with nothing logged get no bar and are never zero-filled.',
    intake_caloriesHow: 'Logged by hand in the Zepp App, synced via advanced data — the official nutrition API is not open yet.',
    intake_protein_gWhat: 'Protein: the total logged for the day, in grams.',
    intake_protein_gChart: 'Drawn as one bar per day; days with nothing logged get no bar and are never zero-filled.',
    intake_protein_gHow: 'Logged by hand in the Zepp App, synced via advanced data — the official nutrition API is not open yet.',
    intake_fat_gWhat: 'Fat: the total logged for the day, in grams.',
    intake_fat_gChart: 'Drawn as one bar per day; days with nothing logged get no bar and are never zero-filled.',
    intake_fat_gHow: 'Logged by hand in the Zepp App, synced via advanced data — the official nutrition API is not open yet.',
    intake_carbs_gWhat: 'Carbs: the total logged for the day, in grams.',
    intake_carbs_gChart: 'Drawn as one bar per day; days with nothing logged get no bar and are never zero-filled.',
    intake_carbs_gHow: 'Logged by hand in the Zepp App, synced via advanced data — the official nutrition API is not open yet.',
  },
);

export const BODY_METRICS = [
  'weight',
  'bmi',
  'body_fat_rate',
  'muscle_mass',
  'body_water_rate',
  'bone_mass',
  'visceral_fat',
  'bmr',
  'height',
  'intake_calories',
  'intake_protein_g',
  'intake_fat_g',
  'intake_carbs_g',
] as const;

export const bodyInfo = (metric: string): { what: string; chart: string; how: string } | null => {
  if (!(BODY_METRICS as readonly string[]).includes(metric)) return null;
  const t = messagesOf(messages) as Record<string, string>;
  return {
    what: t[`${metric}What`],
    chart: t[`${metric}Chart`],
    how: t[`${metric}How`],
  };
};
