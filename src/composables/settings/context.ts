import { inject, provide, type InjectionKey } from 'vue';
import { createAuthFlow, type AuthFlow } from './useAuthFlow';
import { createOfficialAuth, type OfficialAuth } from './useOfficialAuth';
import { createSettingsFeedback, type SettingsFeedback } from './useSettingsFeedback';
import { createUserPrefs, type UserPrefsState } from './useUserPrefs';
import { useCapabilityBoard } from './useCapabilityBoard';

export interface SettingsContext {
  feedback: SettingsFeedback;
  auth: AuthFlow;
  official: OfficialAuth;
  prefs: UserPrefsState;
  capability: ReturnType<typeof useCapabilityBoard>;
}

const SETTINGS_CONTEXT: InjectionKey<SettingsContext> = Symbol('settings-context');

export const provideSettingsContext = (): SettingsContext => {
  const feedback = createSettingsFeedback();
  const context: SettingsContext = {
    feedback,
    auth: createAuthFlow(feedback),
    official: createOfficialAuth(feedback),
    prefs: createUserPrefs(feedback),
    capability: useCapabilityBoard(),
  };
  provide(SETTINGS_CONTEXT, context);
  return context;
};

export const useSettingsContext = (): SettingsContext => {
  const context = inject(SETTINGS_CONTEXT);
  if (!context) throw new Error('useSettingsContext() must be used inside the settings page');
  return context;
};
