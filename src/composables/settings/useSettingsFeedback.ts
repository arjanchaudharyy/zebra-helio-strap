import { ref, type Ref } from 'vue';

export interface SettingsFeedback {
  dataMessage: Ref<string | null>;
  dataError: Ref<string | null>;
}

export const createSettingsFeedback = (): SettingsFeedback => ({
  dataMessage: ref<string | null>(null),
  dataError: ref<string | null>(null),
});
