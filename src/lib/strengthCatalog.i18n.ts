import { defineMessages } from '../i18n';

export const strengthCatalogMessages = defineMessages(
  {
    unknownAction: (code: number) => `#${code}`,
    muscles: {
      chest: 'Chest', back: 'Back', shoulders: 'Shoulders', biceps: 'Biceps', triceps: 'Triceps',
      forearms: 'Forearms', core: 'Core', glutes: 'Glutes', legs: 'Legs',
    },
    exercises: {
      free_exercise: 'Free exercise', bodyweight_squats: 'Bodyweight squats', straight_arm_front_squats: 'Straight-arm front squats',
      barbell_squats: 'Barbell squats', triceps_pushdowns: 'Triceps pushdowns', bent_over_row: 'Bent-over row',
      bench_press: 'Bench press', sit_up_holds: 'Sit-up holds', knee_touch_sit_ups: 'Knee-touch sit-ups',
      dumbbell_flys: 'Dumbbell flyes', burpee_jumps: 'Burpee jumps', burpees: 'Burpees',
      overhead_tricep_extensions: 'Overhead triceps extensions', lying_tricep_extensions: 'Lying triceps extensions', lateral_raises: 'Lateral raises',
      jumping_jacks: 'Jumping jacks', incline_dumbbell_row: 'Incline dumbbell row', bent_over_dumbbell_row: 'Bent-over dumbbell row',
      bent_over_barbell_row: 'Bent-over barbell row', single_arm_dumbbell_row_left: 'Single-arm dumbbell row (L)', single_arm_dumbbell_row_right: 'Single-arm dumbbell row (R)',
      dumbbell_supine_triceps_extensions: 'Dumbbell lying triceps extensions', supine_triceps_extensions: 'Lying barbell triceps extensions', dumbbell_single_arm_triceps_extensions_left: 'Single-arm dumbbell triceps extension (L)',
      seated_bent_over_dumbbell_triceps_extensions: 'Seated bent-over triceps extensions', seated_triceps_press: 'Seated triceps press', smith_machine_squats: 'Smith machine squats',
      dumbbell_bench_press: 'Dumbbell bench press', smith_machine_bench_press: 'Smith machine bench press', dumbbell_single_arm_triceps_extensions_right: 'Single-arm dumbbell triceps extension (R)',
      incline_dumbbell_flys: 'Incline dumbbell flyes', decline_dumbbell_flys: 'Decline dumbbell flyes', v_bar_tricep_pushdowns: 'V-bar triceps pushdowns',
      rope_triceps_pushdowns: 'Rope triceps pushdowns', elastic_band_lateral_raise: 'Band lateral raises', dumbbell_lateral_raise: 'Dumbbell lateral raises',
      dumbbell_front_raise: 'Dumbbell front raises', pectoral_muscles: 'Pectoral muscles', rectus_abdominis: 'Rectus abdominis',
      external_oblique: 'External obliques', upper_back_muscles: 'Upper back muscles', lower_back_muscles: 'Lower back muscles',
      lateral_back_muscles: 'Latissimus dorsi', shoulder_muscles: 'Shoulder muscles', neck_muscles: 'Neck muscles',
      upper_arm_front_muscles: 'Upper-arm front muscles', upper_arm_back_muscles: 'Upper-arm back muscles', front_forearm_muscles: 'Forearm flexors',
      back_forearm_muscles: 'Forearm extensors', hip_muscles: 'Hip muscles', quadriceps: 'Quadriceps',
      hamstrings: 'Hamstrings', front_calf_muscles: 'Anterior calf muscles', back_calf_muscles: 'Calf muscles',
      lat_pulldowns: 'Lat pulldowns', deadlift: 'Deadlift', push_up: 'Push-ups',
      shoulder_press: 'Shoulder press', pull_up: 'Pull-ups', biceps_curl: 'Biceps curls',
      seated_cable_row: 'Seated cable row', upright_row: 'Upright row', straight_arm_pulldown: 'Straight-arm pulldown',
      russian_twist: 'Russian twists', barbell_deadlift: 'Barbell deadlift', assisted_push_up: 'Assisted push-ups',
      dumbbell_push_press: 'Dumbbell push press', dumbbell_upright_row: 'Dumbbell upright row', neck_pulldown: 'Behind-the-neck pulldown',
      chin_up: 'Chin-ups', dumbbell_biceps_curl: 'Dumbbell biceps curls', t_bar_row: 'T-bar row',
      kettlebell_deadlift: 'Kettlebell deadlift', kneeling_push_up: 'Kneeling push-ups', rotational_dumbbell_push_press: 'Rotational dumbbell press',
      incline_barbell_upright_row: 'Incline barbell upright row', rope_straight_arm_pulldown: 'Rope straight-arm pulldown', standard_pull_up: 'Standard pull-ups',
      hammer_curl_to_press: 'Hammer curl to press', seated_row_machine: 'Seated row machine', chest_flyes: 'Chest flyes',
      single_arm_kettlebell_swing_right: 'Single-arm kettlebell swing (R)', barbell_push_press: 'Barbell push press', dumbbell_compound_push_press: 'Dumbbell compound press',
      barbell_biceps_curl: 'Barbell biceps curls', single_arm_kettlebell_swing_left: 'Single-arm kettlebell swing (L)', v_grip_cable_row: 'V-grip cable row',
      dumbbell_front_raise_left: 'Dumbbell front raise (L)', dumbbell_front_raise_right: 'Dumbbell front raise (R)', bent_over_lateral_raise: 'Bent-over lateral raises',
      front_raise: 'Front raises', plank: 'Plank', barbell_hip_thrust: 'Barbell hip thrust',
      barbell_snatch: 'Barbell snatch', standing_calf_raise: 'Standing calf raises', barbell_shrug: 'Barbell shrugs',
      dumbbell_shrug: 'Dumbbell shrugs', farmers_walk_lunge: 'Farmer’s walk lunges', v_up: 'V-ups',
      lateral_slide: 'Lateral slides', battle_rope: 'Battle ropes', kettlebell_swing: 'Kettlebell swings',
      walking_recovery: 'Walking recovery', incline_dumbbell_bench_press: 'Incline dumbbell bench press', incline_barbell_bench_press: 'Incline barbell bench press',
      decline_dumbbell_bench_press: 'Decline dumbbell bench press', decline_barbell_bench_press: 'Decline barbell bench press', pec_deck_fly: 'Pec deck flyes',
      reverse_pec_deck_fly: 'Reverse pec deck flyes', trx_triceps_press: 'TRX triceps press', trx_reverse_flys: 'TRX reverse flyes',
      three_point_chest_push: 'Three-point chest press', incline_cable_flys: 'Incline cable flyes', incline_cable_chest_press: 'Incline cable chest press',
      low_cable_crossover: 'Low cable crossover', cable_crossover: 'Cable crossover', reverse_grip_triceps_pushdown: 'Reverse-grip triceps pushdown',
      cable_crunch: 'Cable crunches', smith_machine_incline_bench_press: 'Smith machine incline bench press', smith_machine_decline_bench_press: 'Smith machine decline bench press',
      chest_press_machine: 'Chest press machine', leverage_incline_chest_press: 'Leverage incline chest press', leverage_decline_chest_press: 'Leverage decline chest press',
      leverage_chest_press: 'Leverage chest press', machine_shoulder_press: 'Machine shoulder press',
    },
  },
);

export type StrengthCatalogShape = typeof strengthCatalogMessages.en;

export const resolveStrengthExerciseLabel = (
  exerciseKey: string | null | undefined,
  actionCode: number,
  catalog: StrengthCatalogShape,
  ...zeppNames: Array<string | null | undefined>
): string => {
  if (exerciseKey && Object.prototype.hasOwnProperty.call(catalog.exercises, exerciseKey)) {
    return catalog.exercises[exerciseKey as keyof StrengthCatalogShape['exercises']];
  }
  // Skip a Zepp name in Chinese (CJK U+4E00 to U+9FFF): the UI is English only.
  const zeppName = zeppNames.find((name) => typeof name === 'string' && name.trim() !== '' && !/[\u4e00-\u9fff]/.test(name));
  return zeppName ? zeppName.trim() : catalog.unknownAction(actionCode);
};

export const resolveStrengthMuscleLabel = (
  muscleGroup: string | null | undefined,
  catalog: StrengthCatalogShape,
): string | null => {
  if (muscleGroup && Object.prototype.hasOwnProperty.call(catalog.muscles, muscleGroup)) {
    return catalog.muscles[muscleGroup as keyof StrengthCatalogShape['muscles']];
  }
  return null;
};
