import { defineMessages } from '../../../i18n';

export const dataCardMessages = defineMessages(
  {
    intro: 'Updates on every sync; a dimmed item doesn’t mean your device lacks it',
    codesHint: (count: number) => `${count} workout ${count === 1 ? 'type wasn’t' : 'types weren’t'} recognized`,
    codesFix: 'Name them',
  },
);
