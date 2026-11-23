import { backend, isDesktop } from './bridge';
import { rangeBounds } from './hourlySteps';
import { SERIES_FETCH_DAYS } from './metricSeries';
import { cached, DEVICE_READ_PREFIXES, peek, type Query } from './readCache';
import { useTrendRange } from '../composables/useTrendRange';
import { deviceImageFor } from './deviceImages';
import { journalPageQueries } from './journalQueries';
import { METRICS as BODY_METRICS } from '../views/body/bodyCards';
import type {
  DailyHeartRateExtreme, DataHealth, DeviceProfile, HeartRateZoneOptions, HeartRatePoint, HourlySteps, MetricSeries, Page, SleepSession, StressPoint, TrainingBalancePoint, Workout,
  WorkoutSeries,
} from '../types';

export const HEART_TREND_METRICS = ['resting_hr', 'hrv', 'hrv_rmssd'] as const;
export const HEART_SERIES_MAX_HOURS = 168;
export const TRAINING_METRICS = ['vo2max', 'training_load', 'lactate_threshold_hr', 'lactate_threshold_pace', 'pai_total'] as const;
export const ACTIVITY_METRICS = ['steps', 'distance', 'active_calories', 'active_minutes'] as const;
export const TRAINING_BALANCE_DAYS = Math.max(28, SERIES_FETCH_DAYS);

const decodeImage = async (src: string): Promise<void> => {
  if (!src || typeof Image === 'undefined') return;
  const image = new Image();
  image.src = src;
  await image.decode().catch(() => undefined);
};

const SLEEP_STREAMS = ['sleep'] as const;
const WORKOUT_STREAMS = ['workouts', 'workout_detail'] as const;

export const pageQuery = {
  metricSeries: (metrics: readonly string[], days = SERIES_FETCH_DAYS): Query<MetricSeries[]> => ({
    key: `metric_series:${days}:${metrics.join(',')}`,
    fetch: () => backend.getMetricSeries([...metrics], days),
  }),
  heartRateSeries: (hours: number): Query<HeartRatePoint[]> => ({
    key: `heart_rate_series:${hours}`,
    fetch: () => backend.getHeartRateSeries(hours),
  }),
  dailyHeartRateExtremes: (days: number): Query<DailyHeartRateExtreme[]> => ({
    key: `daily_hr_extremes:${days}`,
    fetch: () => backend.getDailyHeartRateExtremes(days),
  }),
  stressSeries: (hours: number): Query<StressPoint[]> => ({
    key: `stress_series:${hours}`,
    fetch: () => backend.getStressSeries(hours),
  }),
  trainingBalance: (days: number): Query<TrainingBalancePoint[]> => ({
    key: `training_balance:${days}`,
    fetch: () => backend.getTrainingBalance(days),
  }),
  sleepDetail: (sleepId: string): Query<SleepSession | null> => ({
    key: `sleep_detail:${sleepId}`,
    fetch: () => backend.getSleepDetail(sleepId),
    deps: SLEEP_STREAMS,
  }),
  recentSleep: (limit: number): Query<SleepSession[]> => ({
    key: `recent_sleep:${limit}`,
    fetch: () => backend.getRecentSleep(limit),
    deps: SLEEP_STREAMS,
  }),
  recentWorkouts: (limit: number): Query<Workout[]> => ({
    key: `recent_workouts:${limit}`,
    fetch: () => backend.getRecentWorkouts(limit),
    deps: WORKOUT_STREAMS,
  }),
  workoutDetail: (workoutId: string): Query<Workout | null> => ({
    key: `workout_detail:${workoutId}`,
    fetch: () => backend.getWorkoutDetail(workoutId),
    deps: WORKOUT_STREAMS,
  }),
  deviceProfile: (deviceId?: string, sourceScope?: string): Query<DeviceProfile> => ({
    key: `${DEVICE_READ_PREFIXES[0]}${sourceScope ?? ''}:${deviceId ?? ''}`,
    fetch: async () => {
      const profile: DeviceProfile = await backend.getDeviceProfile({ deviceId, sourceScope });
      await decodeImage(deviceImageFor(profile.kind, profile.image_key));
      return profile;
    },
  }),
  workoutDevice: (workoutId: string): Query<DeviceProfile> => ({
    key: `${DEVICE_READ_PREFIXES[1]}${workoutId}`,
    fetch: async () => {
      const detail = await cached(pageQuery.workoutDetail(workoutId));
      if (!detail) return {};
      return cached(pageQuery.deviceProfile(detail.device_id, detail.source_scope)).catch(() => ({}));
    },
  }),
  workoutSeries: (workoutId: string): Query<WorkoutSeries> => ({
    key: `workout_series:${workoutId}`,
    fetch: () => backend.getWorkoutSeries(workoutId),
    deps: WORKOUT_STREAMS,
  }),
  sleepPage: (limit: number): Query<Page<SleepSession>> => ({
    key: `sleep_page:${limit}:0`,
    fetch: () => backend.getSleepPage(limit, 0),
    deps: SLEEP_STREAMS,
  }),
  workoutPage: (limit: number): Query<Page<Workout>> => ({
    key: `workout_page:${limit}:0`,
    fetch: () => backend.getWorkoutPage(limit, 0),
    deps: WORKOUT_STREAMS,
  }),
  heartRateZones: (days: number): Query<HeartRateZoneOptions> => ({
    key: `heart_rate_zones:${days}`,
    fetch: () => backend.getHeartRateZones(days),
  }),
  dataHealth: (windowDays: number): Query<DataHealth> => ({
    key: `data_health:${windowDays}`,
    fetch: () => backend.getDataHealth(windowDays),
  }),
  hourlySteps: (days = SERIES_FETCH_DAYS): Query<HourlySteps[]> => {
    const whole = rangeBounds(days);
    return {
      key: `hourly_steps:${whole.start}:${whole.end}`,
      fetch: () => backend.getHourlySteps(whole.start, whole.end),
    };
  },
};

export const heartPageQueries = () => [
  pageQuery.heartRateSeries(HEART_SERIES_MAX_HOURS),
  pageQuery.metricSeries(HEART_TREND_METRICS),
  pageQuery.dailyHeartRateExtremes(SERIES_FETCH_DAYS),
] as const;
export const bodyPageQueries = () => [pageQuery.metricSeries(BODY_METRICS), pageQuery.stressSeries(24)] as const;
export const trainingPageQueries = () => [
  pageQuery.metricSeries(TRAINING_METRICS),
  pageQuery.trainingBalance(TRAINING_BALANCE_DAYS),
] as const;
export const heartRateZonesQuery = (rangeDays: number = useTrendRange().value) => pageQuery.heartRateZones(Math.max(30, rangeDays));
export const activityPageQueries = () => [
  pageQuery.metricSeries(ACTIVITY_METRICS),
  pageQuery.hourlySteps(SERIES_FETCH_DAYS),
] as const;
export const sleepPageQueries = (sleepId: string) => [pageQuery.sleepDetail(sleepId), pageQuery.recentSleep(7)] as const;
export const workoutPageQueries = (workoutId: string) => [
  pageQuery.workoutDetail(workoutId),
  pageQuery.workoutSeries(workoutId),
  pageQuery.workoutDevice(workoutId),
] as const;
export const RECENT_RECORDS_LIMIT = 150;
export const recentPageQueries = () => [
  pageQuery.recentSleep(RECENT_RECORDS_LIMIT),
  pageQuery.recentWorkouts(RECENT_RECORDS_LIMIT),
] as const;
export const LIST_PAGE_SIZE = 200;
export const HEALTH_CHECK_DAYS = 90;
export const healthCheckQueries = () => [pageQuery.dataHealth(HEALTH_CHECK_DAYS)] as const;

const ROUTES: Array<[RegExp, (match: RegExpMatchArray) => readonly Query<unknown>[]]> = [
  [/^\/heart\/?$/, heartPageQueries],
  [/^\/body\/?$/, bodyPageQueries],
  [/^\/training\/?$/, () => [...trainingPageQueries(), heartRateZonesQuery()]],
  [/^\/activity\/?$/, activityPageQueries],
  [/^\/sleep\/([^/?#]+)$/, (match) => sleepPageQueries(decodeURIComponent(match[1]))],
  [/^\/workouts\/([^/?#]+)$/, (match) => workoutPageQueries(decodeURIComponent(match[1]))],
  [/^\/recent\/?$/, recentPageQueries],
  [/^\/sleep\/?$/, () => [pageQuery.sleepPage(LIST_PAGE_SIZE)]],
  [/^\/workouts\/?$/, () => [pageQuery.workoutPage(LIST_PAGE_SIZE)]],
  [/^\/health-check\/?$/, healthCheckQueries],
  [/^\/journal\/?$/, journalPageQueries],
];

export const queriesFor = (path: string): readonly Query<unknown>[] => {
  const clean = path.split(/[?#]/)[0];
  for (const [pattern, build] of ROUTES) {
    const match = clean.match(pattern);
    if (match) return build(match);
  }
  return [];
};

export const preloadRoute = (path: string, waitMs = 0): Promise<void> => {
  if (!isDesktop()) return Promise.resolve();
  const queries = queriesFor(path);
  if (!queries.length) return Promise.resolve();
  if (waitMs > 0 && queries.every((query) => peek(query))) return Promise.resolve();
  const all = Promise.allSettled(queries.map((query) => cached(query))).then(() => undefined);
  if (waitMs <= 0) return all;
  return Promise.race([all, new Promise<void>((resolve) => { setTimeout(resolve, waitMs); })]);
};
