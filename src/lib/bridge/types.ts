import type {
  HourlySteps,
  LifeEvent, LifeEventInput,
  PlanDocument, PlanDraftPreview, PlanPublishAction, PlanPublishResult, TrainingPlanState,
  AppStatus,
  DailyHeartRateExtreme,
  Page,
  AiHandoffResult,
  CapabilityOverview,
  CapabilityProbe,
  ExportResult,
  ExportSelection,
  HealthOverview,
  HeartRatePoint,
  StressPoint,
  HeartRateZoneOptions,
  HeartRateZonePreference,
  LoginStatus,
  OfficialStatus,
  MetricSeries,
  BackupManifest,
  BackupVerification,
  CoverageLedger,
  DataHealth,
  PendingRestore,
  RestorePreview,
  WeeklyReport,
  WorkoutInsight,
  IntegrityCheckResult,
  LocalApiStatus,
  SportOption,
  WorkoutCodeLabel,
  ReprocessResult,
  DeviceProfile,
  DeviceProfilesResult,
  SleepSession,
  SleepStageSlice,
  RawPayloadCompaction,
  StorageEstimate,
  SyncReport,
  TrainingBalancePoint,
  UserPrefs,
  Workout,
  WorkoutFitOverride,
  WorkoutSeries,
  StrengthCatalogResult,
} from '../../types';

export type UnlistenFn = () => void;

export type AiTaskCategory =
  | 'workout'
  | 'sleep'
  | 'recovery'
  | 'resting_hr'
  | 'heart_rate'
  | 'training'
  | 'body'
  | 'personal_note'
  | 'attachment';

export type AiTaskDetailLevel = 'summary' | 'standard' | 'detailed';

export interface AiTaskCategoryRange {
  category: AiTaskCategory;
  enabled: boolean;
  days_before: number;
  include_workout_day: boolean;
  excluded_metrics?: string[];
  picked_days?: string[];
}

export interface CardPick {
  key: string;
  category: AiTaskCategory;
  date: string;
}

export interface AiTaskAttachmentRef {
  id: string;
  path: string;
  display_name: string;
  kind: 'pdf' | 'image';
  byte_len: number | null;
  added_at: string;
}

export interface AiTaskAttachmentPublicRef {
  display_name: string;
  kind: 'pdf' | 'image';
  byte_len: number | null;
}

export interface AiTask {
  schema_version: 1;
  id: string;
  title: string;
  template_id: string | null;
  workout_ids: string[];
  categories: AiTaskCategoryRange[];
  detail_level: AiTaskDetailLevel;
  prompt: string;
  personal_note: string;
  attachments: AiTaskAttachmentRef[];
  include_precise_gps: boolean;
  mcp_shared: boolean;
  created_at: string;
  updated_at: string;
}

export interface AiTaskPrepareOptions {
  provider?: string;
  briefText?: string | null;
  promptOverride?: string | null;
  dataFileStem?: string | null;
  promptFileStem?: string | null;
  tokenBudget?: number | null;
  markdownGuide?: string | null;
}

export interface AiTaskMarkdownEstimate {
  approx_tokens: number;
  byte_len: number;
  curve_average_seconds: number | null;
  summarized_workouts: string[];
  over_budget: boolean;
  token_budget: number;
}

export interface AiTaskTemplate {
  schema_version: 1;
  id: string;
  builtin: boolean;
  name: string;
  name_code: string | null;
  categories: AiTaskCategoryRange[];
  detail_level: AiTaskDetailLevel;
  prompt_template: string;
  prompt_code: string | null;
  default_summaries: string[];
  required_series: string[];
  created_at: string;
  updated_at: string;
}

export interface MetricBaseline {
  metric: string;
  latest_date: string;
  latest: number;
  median: number;
  spread: number;
  days: number;
  direction: 'above' | 'below' | 'usual';
  delta: number;
  message_code: 'ui.metric_baseline.above' | 'ui.metric_baseline.below' | 'ui.metric_baseline.usual';
  usual_low?: number | null;
  usual_high?: number | null;
  usual_days?: number;
  long_term?: LongTermNorm | null;
}

export interface LongTermNorm {
  since: string;
  days: number;
  median: number;
  low: number;
  high: number;
  years: { year: number; days: number; median: number; low: number; high: number }[];
}

export interface AiTaskSummary {
  id: string;
  title: string;
  template_id: string | null;
  workout_count: number;
  updated_at: string;
  mcp_shared: boolean;
  pinned?: boolean;
  categories?: string[];
}

export interface AiPromptVersion {
  id: number;
  prompt: string;
  replaced_at: string;
}

export interface AiTaskWorkoutBrief {
  workout_id: string;
  workout_type: string;
  start_time: string;
  end_time: string;
  start_date: string;
  distance_meters: number | null;
  calories: number | null;
  avg_hr: number | null;
  max_hr: number | null;
}


export interface AiTaskCoverage {
  category: AiTaskCategory;
  workout_id: string | null;
  start_date: string;
  end_date: string;
  days_in_range: number;
  days_with_data: number;
  covered_dates?: string[];
  sources: string[];
  units: Record<string, string>;
  metric_days?: Record<string, number>;
  missing: boolean;
  picked_dates?: string[];
}

export type AiTaskAttachmentState = 'ok' | 'missing' | 'changed';

export interface AiTaskAttachmentStatus {
  id: string;
  status: AiTaskAttachmentState;
  byte_len: number | null;
}

export interface AiTaskAttachmentStat {
  path: string;
  exists: boolean;
  byte_len: number | null;
  mtime: string | null;
}

export interface AiTaskIssue {
  code: string;
  message: string;
  params?: Record<string, unknown> | null;
}

export interface AiTaskPreview {
  task_id: string;
  workouts: AiTaskWorkoutBrief[];
  coverage: AiTaskCoverage[];
  attachments: AiTaskAttachmentStatus[];
  estimated_bytes: number;
  warnings: AiTaskIssue[];
  markdown?: AiTaskMarkdownEstimate | null;
}

export interface AiTaskPrepareResult {
  status: 'ready' | 'blocked';
  task_id: string;
  output_dir: string;
  json_path: string | null;
  prompt_path: string | null;
  prompt_text: string;
  byte_len: number;
  copied_attachments?: number;
  md_path?: string | null;
  markdown?: AiTaskMarkdownEstimate | null;
  attachments: AiTaskAttachmentStatus[];
  blocked: AiTaskIssue[];
}

export interface BridgeBackend {
  listLifeEvents(start?: string, end?: string): Promise<LifeEvent[]>;
  saveLifeEvent(input: LifeEventInput): Promise<number>;
  deleteLifeEvent(id: number): Promise<void>;

  readClipboardText(): Promise<string>;
  aiTaskDayStrip(daysBefore: number, end?: string): Promise<import('../../types/timeBridge').DayStripRow[]>;
  aiExchangeList(limit?: number): Promise<import('../../types/timeBridge').AiExchange[]>;
  aiExchangeRename(id: string, title: string): Promise<boolean>;
  aiExchangeDelete(id: string): Promise<boolean>;
  aiProfileSave(note: string): Promise<void>;
  trainingPlanAdherence(from: string, to: string): Promise<import('../../types/timeBridge').AdherenceDay[]>;
  trainingPlanUpdateDraft(id: string, document: PlanDocument): Promise<boolean>;
  trainingPlanHistory(limit?: number): Promise<import('../../types/feedback').PlanHistoryEntry[]>;
  workoutFeedbackPrompt(): Promise<import('../../types/feedback').FeedbackPrompt | null>;
  workoutFeedbackToastDue(): Promise<import('../../types/feedback').FeedbackToastDue | null>;
  workoutFeedbackToastShow(text: import('../../types/feedback').FeedbackToastText): Promise<void>;
  workoutFeedbackGet(workoutId: string): Promise<import('../../types/feedback').WorkoutFeedback | null>;
  workoutFeedbackSave(input: import('../../types/feedback').FeedbackInput): Promise<void>;
  workoutFeedbackDismiss(workoutId: string): Promise<void>;
  workoutFeedbackSnooze(): Promise<string>;
  workoutFeedbackAdjusted(workoutId: string, adjustment: import('../../types/feedback').FeedbackAdjustment): Promise<void>;
  coachNotesList(): Promise<import('../../types/coachNotes').CoachNotes>;
  coachNoteAdd(text: string): Promise<number>;
  coachNoteDecide(id: number, accept: boolean): Promise<void>;
  coachNoteEdit(id: number, text: string): Promise<void>;
  coachNoteDelete(id: number): Promise<void>;
  coachNotesExport(fileStem: string, markdown: string): Promise<string>;
  shoesOverview(): Promise<import('../../types/logbook').ShoeOverview>;
  shoeSave(input: import('../../types/logbook').ShoeInput): Promise<number>;
  shoeDelete(id: number): Promise<void>;
  workoutFeelGet(workoutId: string): Promise<import('../../types/logbook').WorkoutFeel | null>;
  workoutFeelSave(input: import('../../types/logbook').WorkoutFeelInput): Promise<void>;
  injuriesList(): Promise<import('../../types/logbook').Injury[]>;
  injurySave(input: import('../../types/logbook').InjuryInput): Promise<number>;
  injuryDelete(id: number): Promise<void>;
  racesList(): Promise<import('../../types/logbook').Race[]>;
  raceSave(input: import('../../types/logbook').RaceInput): Promise<number>;
  raceDelete(id: number): Promise<void>;
  personalRecords(): Promise<import('../../types/logbook').PersonalRecords>;
  annualRings(): Promise<import('../../types/rings').AnnualRings>;
  ringsSaveImage(fileStem: string, png: number[]): Promise<string>;
  trainingPlanState(): Promise<TrainingPlanState>;
  trainingPlanSaveDraft(document: PlanDocument, pasted: boolean): Promise<string>;
  trainingPlanPreview(id: string): Promise<PlanDraftPreview>;
  trainingPlanDiscard(id: string): Promise<boolean>;
  trainingPlanSetAiPublish(allowed: boolean): Promise<boolean>;
  getMcpSidecar(): Promise<McpSidecar>;
  saveMcpBundle(directory: string): Promise<string>;
  mcpWritesList(): Promise<McpWrite[]>;
  mcpWriteUndo(id: number): Promise<McpWrite[]>;
  setMcpTaskOnly(on: boolean): Promise<boolean>;
  trainingPlanPublish(action: PlanPublishAction, confirmClear: boolean, locale?: string): Promise<PlanPublishResult>;
  getAppStatus(): Promise<AppStatus>;
  verifyAuth(): Promise<AppStatus>;
  clearAuth(): Promise<AppStatus>;
  startFreshLibrary(): Promise<void>;
  manualAuth(appToken: string, userId: string, regionHost: string): Promise<AppStatus>;

  startWebLogin(locale: string): Promise<LoginStatus>;
  cancelWebLogin(): Promise<LoginStatus>;
  getLoginStatus(): Promise<LoginStatus>;
  recoverAdvancedCredentials(storage: 'system' | 'file'): Promise<LoginStatus>;

  startOfficialLogin(): Promise<OfficialStatus>;
  cancelOfficialLogin(): Promise<OfficialStatus>;
  recoverOfficialCredentials(storage: 'system' | 'file'): Promise<OfficialStatus>;
  checkOfficialLoginNow(): Promise<OfficialStatus>;
  getOfficialStatus(): Promise<OfficialStatus>;
  disconnectOfficial(): Promise<OfficialStatus>;

  startHistorySync(days: number): Promise<SyncReport>;
  startIncrementalSync(quick?: boolean, skipProbe?: boolean): Promise<SyncReport>;
  cancelSync(): Promise<void>;
  probeDataCapabilities(): Promise<CapabilityProbe[]>;
  getCapabilityOverview(): Promise<CapabilityOverview>;

  getHealthOverview(): Promise<HealthOverview>;
  getHeartRateSeries(hours?: number): Promise<HeartRatePoint[]>;
  getStressSeries(hours?: number): Promise<StressPoint[]>;
  getMetricSeries(metrics: string[], days: number): Promise<MetricSeries[]>;
  /** Coach (Sarvam): is an API key saved? */
  /** Live strap stream over Bluetooth; events arrive on `zebra://strap`. */
  strapLiveStart(): Promise<void>;
  strapLiveStop(): Promise<void>;
  strapSettings(): Promise<Record<string, { value: number | null; list: number[] | null; choices: number[] | null }>>;
  strapSet(key: string, value: number): Promise<void>;
  strapDetail(day?: string): Promise<StrapDetail>;
  /** Buzz the strap: find it, or a slow breathing pattern. */
  wakeAlarmGet(): Promise<{ on: boolean; time: string }>;
  wakeAlarmSet(on: boolean, time: string): Promise<{ on: boolean; time: string }>;
  strapBuzz(pattern: 'find' | 'breathe' | 'hello' | 'wake'): Promise<void>;
  dailyGoals(): Promise<{ steps: number | null; calories: number | null; activeMinutes: number | null }>;
  strapLiveRecent(minutes: number): Promise<{ t: number; bpm: number; rr?: number[] }[]>;
  /** Save the strap's pairing key from the user's own Zepp account; returns how many were found. */
  strapKeyFetch(): Promise<number>;
  /** Download history straight from the strap now. */
  strapSyncNow(): Promise<string>;
  /** Health watch: recent wrist alerts and the illness early-warning status. */
  strapAlerts(): Promise<{ alerts: { id: string; kind: string; at: number; text: string; level?: string }[]; illness: { state: string; text: string; tempDelta?: number; temp?: number; baseTemp?: number } | null; checkedAt?: number }>;
  /** Daily averages from data downloaded straight from the strap. */
  strapDaily(): Promise<{ date: string; hrvRmssd: number | null; hrvSdnnMs: number | null; skinTempC: number | null; stressAvg: number | null; spo2Avg: number | null; breathRate: number | null; restingHr: number | null; readiness: number | null }[]>;
  aiCoachStatus(): Promise<{ provider: string; model: string; baseUrl: string; keys: string[]; ready: boolean }>;
  aiCoachConfigure(providerId: string, model: string, baseUrl: string, key: string | null): Promise<{ provider: string; model: string; baseUrl: string; keys: string[]; ready: boolean }>;
  aiCoachKeyStatus(): Promise<boolean>;
  /** Save (or, with an empty string, remove) the Sarvam API key. */
  aiCoachSetKey(key: string): Promise<boolean>;
  /** Ask the coach. Only called on a person's click. */
  aiCoachAsk(messages: { role: string; content: string }[], effort?: 'none' | 'low' | 'high'): Promise<{ content: string; model: string }>;
  getHourlySteps(start: string, end: string): Promise<HourlySteps[]>;
  getTrainingBalance(days: number): Promise<TrainingBalancePoint[]>;
  getHeartRateZones(days: number): Promise<HeartRateZoneOptions>;
  setHeartRateZonePreference(
    preference: HeartRateZonePreference,
    days: number,
  ): Promise<HeartRateZoneOptions>;
  getStorageEstimate(days: number): Promise<StorageEstimate>;
  setUserPrefs(retentionDays: number, historySyncDays: number, archiveEnabled?: boolean): Promise<UserPrefs>;
  getUserPrefs(): Promise<UserPrefs>;

  getDailyHeartRateExtremes(days: number): Promise<DailyHeartRateExtreme[]>;
  getRecentSleep(limit?: number): Promise<SleepSession[]>;
  getSleepPage(limit: number, offset: number): Promise<Page<SleepSession>>;
  getSleepDetail(sleepId: string): Promise<SleepSession | null>;
  getSleepNapStages(sleepId: string): Promise<SleepStageSlice[]>;
  getRecentWorkouts(limit?: number): Promise<Workout[]>;
  getWorkoutPage(limit: number, offset: number): Promise<Page<Workout>>;
  getWorkoutDetail(workoutId: string): Promise<Workout | null>;
  getWorkoutSeries(workoutId: string): Promise<WorkoutSeries>;
  getStrengthCatalog(locale: string, refresh?: boolean): Promise<StrengthCatalogResult>;
  setWorkoutTypeOverride(workoutId: string, userOverride?: string | null): Promise<Workout>;
  editWorkouts(workoutIds: string[], changeType: boolean, workoutType: string | null, hidden: boolean | null): Promise<{ id: string; changed: number }>;
  undoWorkoutEdit(id: string): Promise<number>;
  getHiddenWorkoutPage(limit: number, offset: number): Promise<Page<Workout>>;
  getWorkoutDetailCoverage(channel?: WorkoutDetailChannel): Promise<WorkoutDetailCoverage>;
  startWorkoutDetailBackfill(maxRecords: number, retryFailed: boolean, channel?: WorkoutDetailChannel): Promise<WorkoutDetailCoverage>;
  getWorkoutFitOverride(workoutId: string): Promise<WorkoutFitOverride | null>;
  importWorkoutFit(workoutId: string, path: string): Promise<WorkoutFitOverride>;
  removeWorkoutFit(workoutId: string): Promise<boolean>;
  getWorkoutTypeOptions(): Promise<SportOption[]>;
  getUnknownWorkoutCodes(): Promise<WorkoutCodeLabel[]>;
  setWorkoutCodeLabel(zeppType: number, label: string | null): Promise<WorkoutCodeLabel[]>;
  setDeviceModelOverride(deviceKey: string, catalogId: string | null): Promise<void>;
  setDeviceHidden(deviceKey: string, hidden: boolean): Promise<void>;
  getLocalApiStatus(): Promise<LocalApiStatus>;
  setLocalApiEnabled(enabled: boolean): Promise<LocalApiStatus>;
  revealLocalApiToken(): Promise<string>;
  rotateLocalApiToken(): Promise<string>;
  getDeviceProfile(query?: { deviceId?: string; sourceScope?: string }): Promise<DeviceProfile>;
  getDeviceProfiles(refresh?: boolean): Promise<DeviceProfilesResult>;

  reprocessLocalData(): Promise<ReprocessResult>;
  getWorkoutInsight(workoutId: string): Promise<WorkoutInsight>;
  getWeeklyReport(): Promise<WeeklyReport>;
  getDataHealth(windowDays?: number): Promise<DataHealth>;
  startHistoryBackfill(fromDate: string, maxChunks?: number): Promise<CoverageLedger>;
  getCoverageLedger(): Promise<CoverageLedger>;
  resetCoverageLedger(): Promise<CoverageLedger>;
  retryFailedBackfillChunks(): Promise<CoverageLedger>;
  setTrayLocale(locale: string): Promise<void>;
  setAutoSyncPrefs(enabled: boolean, intervalMinutes: number): Promise<void>;
  systemRegionLocale(): Promise<string | null>;
  listBackups(): Promise<BackupManifest[]>;
  createManualBackup(): Promise<BackupManifest>;
  verifyBackup(backupId: string): Promise<BackupVerification>;
  setBackupPinned(backupId: string, pinned: boolean): Promise<BackupManifest>;
  getRestorePreview(backupId: string): Promise<RestorePreview>;
  stageRestore(backupId: string): Promise<PendingRestore>;
  getPendingRestore(): Promise<PendingRestore | null>;
  cancelPendingRestore(): Promise<void>;
  runDatabaseIntegrityCheck(): Promise<IntegrityCheckResult>;
  compactRawPayloads(): Promise<RawPayloadCompaction>;
  exportMyRecords(path: string): Promise<number>;
  importWorkouts(source: import('../../types/imports').ImportSourceKey, paths: string[]): Promise<import('../../types/imports').ImportReport>;
  importBatches(): Promise<import('../../types/imports').ImportBatch[]>;
  undoImportBatch(batchId: number): Promise<number>;
  workoutImportSources(): Promise<Record<string, import('../../types/imports').ImportSourceKey>>;
  exportForUnstrap(days: number | null): Promise<import('../../types/imports').UnstrapExportResult>;
  copyWorkoutGpx(workoutId: string): Promise<void>;
  saveFitExport(selection: ExportSelection, directory: string): Promise<ExportResult>;
  prepareAiHandoff(
    selection: ExportSelection,
    prompt: string,
    includePreciseRoute?: boolean,
  ): Promise<AiHandoffResult>;
  openExternalUrl(url: string): Promise<void>;


  aiTaskList(): Promise<AiTaskSummary[]>;
  aiTaskGet(id: string): Promise<AiTask>;
  aiTaskSave(task: AiTask): Promise<AiTask>;
  aiTaskDelete(id: string): Promise<void>;
  aiTaskDeleteMany(ids: string[]): Promise<Array<{ task: AiTask; pinned: boolean }>>;
  getMetricBaselines(metrics: string[]): Promise<MetricBaseline[]>;
  metricExclusions(): Promise<string[]>;
  setMetricExcluded(metric: string, excluded: boolean): Promise<string[]>;
  aiTaskSetPinned(id: string, pinned: boolean): Promise<void>;
  aiPromptVersions(id: string): Promise<AiPromptVersion[]>;
  aiPromptExport(id: string): Promise<string>;
  aiPromptImport(path: string): Promise<AiTask>;
  cardCollectionGet(): Promise<CardPick[]>;
  cardCollectionSet(picks: CardPick[]): Promise<CardPick[]>;
  aiTemplateList(): Promise<AiTaskTemplate[]>;
  aiTemplateSave(template: AiTaskTemplate): Promise<AiTaskTemplate>;
  aiTemplateDelete(id: string): Promise<void>;
  aiTaskPreview(task: AiTask, tokenBudget?: number | null): Promise<AiTaskPreview>;
  aiTaskPrepare(task: AiTask, coverageNote: string, directionText?: string | null, options?: AiTaskPrepareOptions): Promise<AiTaskPrepareResult>;
  aiTaskAttachmentStat(paths: string[]): Promise<AiTaskAttachmentStat[]>;
  aiHandoffCopyFile(mdPath: string): Promise<void>;
  aiHandoffAutopaste(request: { provider: string; mdPath: string; kickoff: string; send: boolean }): Promise<number>;
  aiHandoffCancel(): Promise<void>;

  cleanupOldData(days: number): Promise<Record<string, unknown>>;
  openDataFolder(): Promise<void>;
  cloudCompanionState(): Promise<CloudCompanionState>;
  cloudCompanionSaveConfig(input: CloudConfigInput): Promise<CloudCompanionState>;
  cloudCompanionUpload(): Promise<CloudCompanionState>;
  cloudCompanionSetup(command: CloudSetupCommand, label?: string): Promise<CloudSetupOutcome>;
  cloudCompanionOpen(target: 'site' | 'logs' | 'folder'): Promise<void>;
  pushState(): Promise<PushState>;
  pushSave(input: PushSaveInput): Promise<PushState>;
  pushSetEnabled(enabled: boolean): Promise<PushState>;
  pushImportCompanion(): Promise<PushState>;
  pushRun(): Promise<PushState>;
  pushRemove(): Promise<PushState>;

  listen<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn>;
}

export interface CloudConfig {
  site: string;
  bin_dir: string | null;
  data_dir: string | null;
  sync: boolean;
  retention_days: number;
  summary_days: number;
  full_resolution_workouts: boolean;
}

export interface CloudConfigInput extends CloudConfig {
  uploader_dir: string | null;
}

export interface CloudLastRun {
  at: string;
  mode: string;
  result: string;
  exit_code: number;
  log_line: string;
}

export interface CloudReader {
  label: string;
  created_at: string | null;
}

export interface CloudCompanionState {
  supported: boolean;
  configured: boolean;
  config_unreadable: boolean;
  state_dir: string | null;
  uploader_dir: string | null;
  config: CloudConfig | null;
  site_url: string | null;
  last_run: CloudLastRun | null;
  readers: CloudReader[];
  task_follow: boolean;
  task_upload: boolean;
  running: boolean;
  default_bin_dir: string | null;
}

export interface PushLast {
  /** ok / unchanged / skipped_locked / failed */
  result: string;
  stats: { objects: number; deleted: number; unchanged: number; bytes_sent: number };
  error_code: string | null;
  at: string;
}

export interface PushState {
  configured: boolean;
  enabled: boolean;
  site: string | null;
  site_url: string | null;
  has_secrets: boolean;
  credential_store_ok: boolean;
  last: PushLast | null;
  running: boolean;
  config_error_code: string | null;
  companion_site_url: string | null;
  companion_tasks: boolean;
}

export interface PushSaveInput {
  site: string;
  connection_code: string | null;
  enabled: boolean;
}

export type CloudSetupCommand = 'init' | 'task-commands' | 'add-reader' | 'copy-reader' | 'revoke-reader';

export interface CloudSetupOutcome {
  state: CloudCompanionState;
  output: string | null;
}

export interface McpSidecar {
  path: string | null;
  data_dir_env: string | null;
  appimage?: boolean;
  stale_clients?: McpClientId[];
  cli_path?: string | null;
  scope_stale_clients?: McpClientId[];
  task_only?: boolean;
  outdated?: boolean;
}
export type McpClientId = 'claude_desktop' | 'claude_code' | 'codex' | 'opencode' | 'grok' | 'factory' | 'qwen' | 'gemini';

export interface McpWrite {
  id: number;
  tool: string;
  kind: 'injury' | 'life_event' | 'race' | 'shoe' | 'workout_feel' | 'workout_type' | 'coach_note';
  target_id: string;
  label: string;
  action: 'created' | 'updated';
  source_name: string | null;
  created_at: string;
  undone_at: string | null;
}

export type WorkoutDetailChannel = 'advanced' | 'official';

export interface WorkoutDetailCoverage {
  pending: number; persisted: number; empty_from_cloud: number; request_failed: number;
  parse_failed: number; unavailable: number; ready: number;
}

export interface StrapDetail {
  day: string;
  heartRate: { t: number; bpm: number }[];
  heartRateReadings: number;
  steps: { t: number; steps: number }[];
  totalSteps: number;
  wornMinutes: number;
  stress: { t: number; v: number }[];
  pai: { at: number; daily: number | null; total: number | null; lowMinutes: number | null; mediumMinutes: number | null; highMinutes: number | null; lowFrom: number | null; mediumFrom: number | null; highFrom: number | null; restHr: number | null; maxHr: number | null } | null;
  breathing: { night: number; score: number | null; hoursChecked: number | null; dips: number | null }[];
  nightOxygen: { min: number; avg: number; readings: number } | null;
  skinTemp?: { t: number; c: number }[];
  minutes?: { sit: number; stand: number; walk: number; run: number; fatBurn: number };
  floors?: number;
  distanceM?: number;
}
