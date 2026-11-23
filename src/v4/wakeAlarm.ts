/* Wake-up alarm and bedtime reminder. The backend checks the times and buzzes the strap (see strap_live.rs). */
import { ref } from 'vue';
import { backend } from '../lib/bridge';

export interface Alarms { on: boolean; time: string; bedOn: boolean; bedTime: string }
export const alarms = ref<Alarms>({ on: false, time: '07:00', bedOn: false, bedTime: '22:30' });
const norm = (a: { on: boolean; time: string; bedOn?: boolean; bedTime?: string }): Alarms => ({ on: a.on, time: a.time, bedOn: a.bedOn ?? false, bedTime: a.bedTime ?? '22:30' });
export const loadAlarms = async () => { alarms.value = norm(await backend.wakeAlarmGet().catch(() => alarms.value)); };
export const saveAlarms = async (patch: Partial<Alarms>) => {
  const next = { ...alarms.value, ...patch };
  alarms.value = next;
  alarms.value = norm(await backend.wakeAlarmSet(next.on, next.time, next.bedOn, next.bedTime).catch(() => next));
};
