import { computed } from 'vue';
import { defineMessages, useMessages } from '../../i18n';
import { splitPart } from '../../lib/logbook/body';

const messages = defineMessages(
  {
    front: 'Front', back: 'Back', clear: 'Clear', none: 'Nothing picked',
    figure: 'Body figure: pick the spots',
    sided: (base: string, side: 'l' | 'r', _key: string) => `${side === 'l' ? 'Left' : 'Right'} ${base}`,
    parts: {
      neck: 'Neck', shoulder: 'shoulder', upper_back: 'Upper back', lower_back: 'Lower back', chest: 'Chest', abdomen: 'Abdomen',
      hip: 'hip flexor', glute: 'glute', quad: 'quad', hamstring: 'hamstring', knee: 'knee', shin: 'shin',
      calf: 'calf', achilles: 'Achilles', ankle: 'ankle', foot: 'foot', it_band: 'IT band',
      head: 'Head', upper_arm: 'upper arm', elbow: 'elbow', forearm: 'forearm', wrist: 'wrist', hand: 'hand',
    },
  },
);

export const useBodyText = () => useMessages(messages);

export const useBodyPartName = () => {
  const t = useBodyText();
  return computed(() => (part: string) => {
    const [base, side] = splitPart(part);
    const name = (t.value.parts as Record<string, string>)[base] ?? part;
    return side ? t.value.sided(name, side, base) : name;
  });
};
