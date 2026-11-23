import { defineMessages, messagesOf } from '../i18n';
import { workoutNameMessages } from './workoutNames.i18n';


const messages = defineMessages(
  {
    unknownWithCode: (code: string) => `Unrecognized workout (code ${code})`,
    workout: 'Workout',
    fallback: {
      run: 'Outdoor Running',
      running: 'Running',
      walking: 'Walking',
      walk: 'Walk',
      ride: 'Outdoor Cycling',
      cycling: 'Outdoor Cycling',
      indoor_cycling: 'Indoor Cycling',
      swimming: 'Swimming',
      treadmill: 'Treadmill',
      indoor_run: 'Indoor Running',
      trail: 'Trail Running',
      hiking: 'Hiking',
      strength: 'Strength Training',
      elliptical: 'Elliptical',
      rowing: 'Rowing',
      yoga: 'Yoga',
      climb: 'Climbing',
      badminton: 'Badminton',
      activity: 'Activity',
      unknown: 'Unrecognized workout',
    },
    providerZeppCloud: 'Zepp Cloud',
    scopeUserFused: 'User-fused',
    scopeDevice: 'Single device',
    scopeMixed: 'Multiple sources',
    scopeUnknown: 'Scope unconfirmed',
  },
);

const copy = () => messagesOf(messages);

export const workoutLabel = (value: string): string => {
  const t = copy();
  const unknownCode = value.trim().toLowerCase().match(/^unknown:(-?\d+)$/);
  if (unknownCode) return t.unknownWithCode(unknownCode[1]);
  const normalized = value.trim().toLowerCase();
  const fallback = t.fallback as Record<string, string | undefined>;
  const names = messagesOf(workoutNameMessages) as Record<string, string | undefined>;
  return names[normalized] || fallback[normalized] || value || t.workout;
};


export const dataProviderLabel = (): string => copy().providerZeppCloud;

export const dataScopeLabel = (scope?: string): string => {
  const t = copy();
  if (scope === 'user_fused') return t.scopeUserFused;
  if (scope === 'device') return t.scopeDevice;
  if (scope === 'mixed') return t.scopeMixed;
  return t.scopeUnknown;
};
