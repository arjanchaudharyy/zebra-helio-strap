const SHARED = new Set([
  'get_app_status',
  'get_coverage_ledger',
  'get_storage_estimate',
  'list_backups',
  'get_pending_restore',
  'get_unknown_workout_codes',
  'get_local_api_status',
]);

const READ_ONLY = /^(get_|list_|plugin:event\||plugin:app\||app_is_ready$|ai_task_list$|ai_exchange_list$|training_plan_state$|shoes_overview$|workout_feel_get$|injuries_list$|races_list$|personal_records$|annual_rings$)/;

export const REUSE_MS = 1000;

type Flight = { epoch: number; settledAt: number | null; promise: Promise<unknown> };

let epoch = 0;
const flights = new Map<string, Flight>();

export const stateChanged = () => {
  epoch += 1;
  flights.clear();
};

export const sharedCall = <T>(command: string, args: Record<string, unknown> | undefined, send: () => Promise<T>): Promise<T> => {
  if (!SHARED.has(command)) {
    const result = send();
    if (!READ_ONLY.test(command)) void result.then(stateChanged, stateChanged);
    return result;
  }
  const key = `${command}:${args ? JSON.stringify(args) : ''}`;
  const now = Date.now();
  const hit = flights.get(key);
  if (hit && hit.epoch === epoch && (hit.settledAt === null || now - hit.settledAt < REUSE_MS)) {
    return hit.promise.then((value) => structuredClone(value)) as Promise<T>;
  }
  const flight: Flight = { epoch, settledAt: null, promise: Promise.resolve() };
  flight.promise = send().then(
    (value) => {
      flight.settledAt = Date.now();
      return value;
    },
    (cause: unknown) => {
      if (flights.get(key) === flight) flights.delete(key);
      throw cause;
    },
  );
  flights.set(key, flight);
  return flight.promise as Promise<T>;
};

export const resetSharedReads = () => {
  epoch = 0;
  flights.clear();
};
