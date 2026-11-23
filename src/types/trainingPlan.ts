
export interface PlanRestInput { date: string; bedtime?: string; sleepTarget?: string; note?: string }
export interface PlanRestDay { date: string; bedtime_minutes: number | null; sleep_target_seconds: number | null; note: string | null }
export interface PlanDocument {
  format?: string;
  summary?: string;
  rest?: PlanRestInput[];
  from?: string;
  to?: string;
  workouts: PlanWorkoutInput[];
}

export interface PlanWorkoutInput {
  date: string;
  sport: string;
  variant?: string;
  name: string;
  focus?: string;
  description?: string;
  steps: PlanStepInput[];
}

export type PlanStepInput =
  | { repeat: number; steps: PlanStepInput[] }
  | { kind: string; duration: string; target?: string; note?: string };

export type PlanSport = 'running' | 'cycling' | 'pool_swim' | 'open_water_swim';
export type PlanVariant = 'outdoor' | 'indoor' | 'treadmill' | 'track' | 'trail' | 'mountain';
export type PlanActivity = 'walking' | 'hiking' | 'strength' | 'yoga' | 'rowing' | 'elliptical' | 'inline_skating' | 'cross_country_skiing';
export type PlanIntensity = 'warmup' | 'active' | 'interval' | 'recovery' | 'rest' | 'cooldown';

export type PlanStepLength = { type: 'time'; seconds: number } | { type: 'distance'; meters: number };

export type PlanTarget =
  | { type: 'open' }
  | { type: 'heart_rate'; low: number; high: number }
  | { type: 'pace'; fast: number; slow: number }
  | { type: 'power'; low: number; high: number };

export interface PlanStep {
  intensity: PlanIntensity;
  length: PlanStepLength;
  target: PlanTarget;
  note?: string;
}

export type PlanStepNode =
  | ({ type: 'step' } & PlanStep)
  | { type: 'repeat'; times: number; steps: PlanStep[] };

export interface PlanWorkout {
  date: string;
  sport: PlanSport;
  variant?: PlanVariant;
  name: string;
  focus?: string;
  description?: string;
  steps: PlanStepNode[];
}

export interface PlanHeldWorkout {
  index: number;
  date: string;
  sport: string;
  activity?: PlanActivity;
  name: string;
  focus?: string;
  description?: string;
  steps: PlanStepNode[];
}

export interface PlanIssue {
  severity: 'error' | 'unverified' | 'warning';
  workout?: number;
  step?: string;
  message: string;
  message_code: string;
  params: Record<string, unknown>;
}

export interface PlanCheck {
  summary?: string | null;
  rest?: PlanRestDay[];
  from: string | null;
  to: string | null;
  workouts: PlanWorkout[];
  held?: PlanHeldWorkout[];
  issues: PlanIssue[];
}

export type PlanDayChange = 'rest' | 'unchanged' | 'added' | 'replaced' | 'removed';

export interface PlanDayPreview {
  rest?: PlanRestDay | null;
  date: string;
  change: PlanDayChange;
  before: PlanWorkout[];
  after: PlanWorkout[];
}

export interface PlanWindow {
  start: string;
}

export interface PlanDraftPreview {
  check: PlanCheck;
  window: PlanWindow;
  days: PlanDayPreview[];
}

export interface PlanDraft {
  id: string;
  origin: 'user' | 'ai_paste' | 'mcp';
  status: string;
  document: PlanDocument;
  created_at: string;
  updated_at: string;
}

export interface PlanPublishRecord {
  id: number;
  batch_id: number;
  kind: 'publish' | 'roll' | 'undo' | 'clear' | 'revoked';
  role: 'window' | 'clear' | 'placeholder';
  window_start: string;
  workout_count: number;
  state: 'pending' | 'sent' | 'rejected' | 'unknown' | 'partial';
  http_status: number | null;
  error_code: string | null;
  undone: boolean;
  created_at: string;
  finished_at: string | null;
}

export interface TrainingPlanState {
  window: PlanWindow;
  sent: PlanWorkout[];
  planned: PlanWorkout[];
  uncertain: boolean;
  last_publish: PlanPublishRecord | null;
  last_batch: PlanPublishRecord[];
  weeks: PlanWeekStatus[];
  can_undo: boolean;
  drafts: PlanDraft[];
  ai_may_publish: boolean;
}

export interface PlanWeekStatus {
  start: string;
  planned: number;
  sent: number;
  state: 'in_sync' | 'uncertain' | 'not_sent';
  error_code: string | null;
}

export type PlanPublishAction = { kind: 'draft'; id: string } | { kind: 'undo' } | { kind: 'clear' } | { kind: 'resend' };

export type PlanPrepared =
  | { outcome: 'send'; batch_id: number; sends: { publish_id: number; window_start: string; role: PlanPublishRecord['role'] }[] }
  | { outcome: 'not_needed' }
  | { outcome: 'needs_clear_confirmation' }
  | { outcome: 'invalid'; check: PlanCheck }
  | { outcome: 'nothing_to_undo' };

export interface PlanPublishResult {
  outcome: PlanPrepared;
  record: PlanPublishRecord | null;
  records: PlanPublishRecord[];
}
