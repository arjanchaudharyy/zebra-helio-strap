
export type ShoeSport = 'running' | 'trail' | 'walking' | 'hiking';
export type ShoeWear = 'fresh' | 'near' | 'due' | 'retired';
export type ShoeHow = 'chosen' | 'none' | 'zepp' | 'default' | 'unknown';

export interface ShoeInput {
  id: number | null;
  name: string;
  sport: ShoeSport;
  is_default: boolean;
  zepp_gear_id: string | null;
  start_km: number;
  retire_km: number;
  started_on: string;
  retired_on: string | null;
  notes: string;
}

export interface ShoeSummary extends ShoeInput {
  id: number;
  created_at: string;
  updated_at: string;
  km: number;
  workouts: number;
  last_used: string | null;
  wear: ShoeWear;
}

export interface UnknownGear {
  zepp_gear_id: string;
  workouts: number;
  km: number;
  first_used: string;
  last_used: string;
  sport: ShoeSport | null;
}

export interface ShoeOverview {
  shoes: ShoeSummary[];
  unknown_gear: UnknownGear[];
}

export interface ShoeAssignment {
  shoe_id: number | null;
  how: ShoeHow;
}

export interface ShoeChoice {
  id: number;
  name: string;
  sport: ShoeSport;
  retired: boolean;
}

export type ShoeMode = 'auto' | 'none' | 'chosen';

export interface WorkoutFeelInput {
  workout_id: string;
  rpe: number | null;
  soreness: string[];
  shoe_mode: ShoeMode;
  shoe_id: number | null;
}

export interface WorkoutFeel extends WorkoutFeelInput {
  watch_rpe: number | null;
  footwear: ShoeSport | null;
  assigned: ShoeAssignment | null;
  choices: ShoeChoice[];
}

export interface InjuryInput {
  id: number | null;
  title: string;
  body_part: string | null;
  severity: 1 | 2 | 3 | null;
  start_date: string;
  end_date: string | null;
  notes: string;
}

export interface Injury extends InjuryInput {
  id: number;
  created_at: string;
  updated_at: string;
}

export type RaceSport = 'running' | 'trail' | 'cycling' | 'swimming' | 'triathlon' | 'other';
export type RacePriority = 'a' | 'b' | 'c';

export interface RaceInput {
  id: number | null;
  name: string;
  race_date: string;
  sport: RaceSport;
  distance_m: number | null;
  goal_seconds: number | null;
  priority: RacePriority;
  result_seconds: number | null;
  workout_id: string | null;
  notes: string;
}

export interface RaceCandidate {
  workout_id: string;
  start_time: string;
  elapsed_seconds: number;
  distance_m: number | null;
}

export interface Race extends RaceInput {
  id: number;
  created_at: string;
  updated_at: string;
  candidate: RaceCandidate | null;
}

export type RecordKey = '5k' | '10k' | 'half' | 'marathon';

export interface Effort {
  seconds: number;
  method: 'samples' | 'splits' | 'whole' | 'race';
  workout_id: string | null;
  race_id: number | null;
  race_name: string | null;
  date: string;
}

export interface RecordRow {
  key: RecordKey;
  distance_m: number;
  best: Effort | null;
  runners_up: Effort[];
}

export interface PersonalRecords {
  records: RecordRow[];
  runs_scanned: number;
}
