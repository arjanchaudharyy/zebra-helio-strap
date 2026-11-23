import type { RecordKey } from './logbook';

export interface RingStar {
  workout_id: string;
  date: string;
  kind: string;
  is_run: boolean;
  distance_m: number | null;
  duration_s: number | null;
}

export interface RingHighlight {
  workout_id: string;
  date: string;
  kind: string;
  distance_m: number | null;
  duration_s: number | null;
}

export interface RingRace {
  name: string;
  date: string;
  result_seconds: number;
}


export interface RingYear {
  year: number;
  in_progress: boolean;
  data_days: number;
  workouts: number;
  active_days: number;
  duration_s: number;
  distance_m: number;
  runs: number;
  run_distance_m: number;
  weeks: (number | null)[];
  busiest_month: number | null;
  busiest_month_workouts: number;
  longest_run: RingHighlight | null;
  longest_workout: RingHighlight | null;
  steps_total: number | null;
  steps_days: number;
  sleep_avg_minutes: number | null;
  sleep_nights: number;
  personal_records: RecordKey[];
  races: RingRace[];
  stars: RingStar[];
}

export interface AnnualRings {
  years: RingYear[];
  today: string;
}
