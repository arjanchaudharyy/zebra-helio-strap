import { defineMessages, useMessages } from '../../i18n';

const messages = defineMessages(
  {
    front: 'Front',
    back: 'Back',
    figureAria: (side: string) => `Muscle load figure · ${side}`,
    muscles: {
      chest: 'Chest', obliques: 'Obliques', abs: 'Abs', biceps: 'Biceps', triceps: 'Triceps',
      neck: 'Neck', trapezius: 'Trapezius', deltoids: 'Deltoids', adductors: 'Adductors', quadriceps: 'Quadriceps',
      knees: 'Knee', tibialis: 'Tibialis', calves: 'Calves', forearm: 'Forearm', hands: 'Hand', ankles: 'Ankle',
      feet: 'Foot', head: 'Head', hair: 'Head', 'upper-back': 'Lats', 'lower-back': 'Lower back', gluteal: 'Glutes',
      hamstring: 'Hamstrings',
    },
    loadTitle: (name: string, primary: number, secondary: number) =>
      [name, primary > 0 ? `primary ${primary} ${primary === 1 ? 'set' : 'sets'}` : '', secondary > 0 ? `secondary ${secondary} ${secondary === 1 ? 'set' : 'sets'}` : ''].filter(Boolean).join(' · '),
    legendLight: 'Fewer sets',
    legendHeavy: 'More sets',
    legendSecondary: 'Secondary only',
  },
);

export const anatomyMessages = messages;
export const useAnatomyText = () => useMessages(messages);
