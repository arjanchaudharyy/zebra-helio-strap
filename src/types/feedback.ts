import type { PlanSport, PlanWorkout, PlanDraft, PlanPublishRecord } from './trainingPlan';

export type FeedbackRating = 1 | 2 | 3 | 4 | 5;
export type FeedbackTag = 'fatigue' | 'pain' | 'weather';
export type FeedbackAdjustment = 'lighter_10';

export interface FeedbackPlanned {
  plan_workout_id: number;
  name: string;
  sport: PlanSport;
  seconds: number | null;
  meters: number | null;
  hr_low: number | null;
  hr_high: number | null;
}

export interface FeedbackActual {
  workout_id: string;
  sport: string;
  start_time: string;
  end_time: string;
  seconds: number | null;
  meters: number | null;
  avg_hr: number | null;
}

export interface FeedbackPrompt {
  date: string;
  planned: FeedbackPlanned;
  actual: FeedbackActual;
  completion: number | null;
  completion_basis: 'duration' | 'distance' | null;
  expires_at: string;
}

export interface FeedbackInput {
  workout_id: string;
  rating: FeedbackRating;
  tags: FeedbackTag[];
  note?: string | null;
}

export interface WorkoutFeedback {
  workout_id: string;
  plan_workout_id: number | null;
  state: 'rated' | 'dismissed';
  rating: FeedbackRating | null;
  tags: FeedbackTag[];
  note: string | null;
  adjustment: FeedbackAdjustment | null;
  created_at: string;
  updated_at: string;
  start_time: string | null;
  sport: string | null;
  plan_name: string | null;
  plan_date: string | null;
}

export interface PlanHistoryWorkout extends PlanWorkout {
  plan_workout_id: number;
  rating: FeedbackRating | null;
}

export interface PlanHistoryEntry {
  record: PlanPublishRecord;
  origin: PlanDraft['origin'] | null;
  summary: string | null;
  workouts: PlanHistoryWorkout[];
  replaced: number;
}

export interface FeedbackToastDue {
  token: string;
  prompt: FeedbackPrompt;
}

export interface FeedbackToastText {
  token: string;
  title: string;
  body: string;
  labels: string[];
}
