import type { AnatomyMarker, AnatomySide, AnatomySlug, AnatomyView } from '../anatomy/types';

export const BODY_PARTS = [
  'neck', 'shoulder_l', 'shoulder_r', 'upper_back', 'lower_back', 'chest', 'abdomen',
  'hip_l', 'hip_r', 'glute_l', 'glute_r', 'quad_l', 'quad_r', 'hamstring_l', 'hamstring_r',
  'knee_l', 'knee_r', 'shin_l', 'shin_r', 'calf_l', 'calf_r', 'achilles_l', 'achilles_r',
  'ankle_l', 'ankle_r', 'foot_l', 'foot_r', 'it_band_l', 'it_band_r',
  'head', 'upper_arm_l', 'upper_arm_r', 'elbow_l', 'elbow_r', 'forearm_l', 'forearm_r',
  'wrist_l', 'wrist_r', 'hand_l', 'hand_r',
] as const;
export type BodyPart = (typeof BODY_PARTS)[number];
export const splitPart = (part: string): [string, 'l' | 'r' | null] => {
  const match = /^(.*)_(l|r)$/.exec(part);
  return match ? [match[1], match[2] as 'l' | 'r'] : [part, null];
};

export interface BodyPartPlace {
  shapes: Array<{ slug: AnatomySlug; side: AnatomySide }>;
  markers: AnatomyMarker[];
}

const MIRROR: Record<AnatomyView, number> = { front: 728, back: 2167 };
const LEFT_MARKERS: Record<string, Array<[AnatomyView, number, number, number, number]>> = {
  elbow: [['front', 528, 497, 15, 15], ['back', 912, 527, 15, 15]],
  wrist: [['front', 600, 690, 12, 12], ['back', 850, 690, 12, 12]],
  hip: [['front', 466, 655, 20, 15]],
  it_band: [['front', 481, 800, 9, 70], ['back', 966, 850, 9, 75]],
  achilles: [['back', 1004, 1240, 9, 24]],
};
const SLUGS: Record<string, AnatomySlug[]> = {
  head: ['head', 'hair'], neck: ['neck'], chest: ['chest'], abdomen: ['abs', 'obliques'],
  upper_back: ['trapezius', 'upper-back'], lower_back: ['lower-back'], shoulder: ['deltoids'],
  upper_arm: ['biceps', 'triceps'], forearm: ['forearm'], hand: ['hands'], glute: ['gluteal'],
  quad: ['quadriceps'], hamstring: ['hamstring'], knee: ['knees'], shin: ['tibialis'],
  calf: ['calves'], ankle: ['ankles'], foot: ['feet'],
};

export const BODY_PART_PLACES: Readonly<Record<BodyPart, BodyPartPlace>> = Object.fromEntries(
  BODY_PARTS.map((part) => {
    const [base, side] = splitPart(part);
    const sides: AnatomySide[] = side ? [side] : ['l', 'r', 'c'];
    const shapes = (SLUGS[base] ?? []).flatMap((slug) => sides.map((s) => ({ slug, side: s })));
    const markers = (LEFT_MARKERS[base] ?? []).map(([view, cx, cy, rx, ry]) => ({
      id: `${part}-${view}`, view, cx: side === 'r' ? MIRROR[view] - cx : cx, cy, rx, ry,
    }));
    return [part, { shapes, markers }];
  }),
) as Record<BodyPart, BodyPartPlace>;

export const isBodyPart = (value: string): value is BodyPart => (BODY_PARTS as readonly string[]).includes(value);
