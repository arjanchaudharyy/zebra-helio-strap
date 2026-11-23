import { defineMessages, messagesOf } from '../i18n';

const messages = defineMessages(
  { fused: 'Combined sources', undetermined: 'Device undetermined' },
);

export interface DeviceNameInput {
  readonly canonical_name?: string;
  readonly name?: string;
  readonly name_code?: string;
}

export const deviceNameText = (profile: DeviceNameInput | null | undefined): string => {
  if (!profile) return '';
  const named = profile.canonical_name || profile.name;
  if (named) return named;
  const t = messagesOf(messages);
  switch (profile.name_code) {
    case 'ui.device.fused': return t.fused;
    case 'ui.device.undetermined': return t.undetermined;
    default: return '';
  }
};

export const undeterminedDevice = (): DeviceNameInput => ({ name_code: 'ui.device.undetermined' });
