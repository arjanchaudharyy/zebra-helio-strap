/* Wake-up alarm settings. The backend checks the time and buzzes the strap (see strap_live.rs). */
import { ref } from 'vue';
import { backend } from '../lib/bridge';

export const wakeAlarm = ref<{ on: boolean; time: string }>({ on: false, time: '07:00' });
export const loadWakeAlarm = async () => { wakeAlarm.value = await backend.wakeAlarmGet().catch(() => wakeAlarm.value); };
export const saveWakeAlarm = async (on: boolean, time: string) => {
  wakeAlarm.value = { on, time };
  wakeAlarm.value = await backend.wakeAlarmSet(on, time).catch(() => wakeAlarm.value);
};
