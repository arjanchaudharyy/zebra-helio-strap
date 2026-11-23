
export type AnatomyView = 'front' | 'back';
export type AnatomySide = 'l' | 'r' | 'c';

export type AnatomySlug =
  | 'chest' | 'obliques' | 'abs' | 'biceps' | 'triceps' | 'neck' | 'trapezius' | 'deltoids'
  | 'adductors' | 'quadriceps' | 'knees' | 'tibialis' | 'calves' | 'forearm' | 'hands' | 'ankles'
  | 'feet' | 'head' | 'hair' | 'upper-back' | 'lower-back' | 'gluteal' | 'hamstring';

export interface AnatomyShape {
  slug: AnatomySlug;
  side: AnatomySide;
  d: string;
}

export interface AnatomyMarker {
  id: string;
  view: AnatomyView;
  cx: number;
  cy: number;
  rx: number;
  ry: number;
}

export interface AnatomyRegionState {
  level?: 0 | 1 | 2 | 3;
  hatched?: boolean;
  selected?: boolean;
  marked?: boolean;
  dim?: boolean;
  hot?: boolean;
}

export interface AnatomyRegion {
  key: string;
  label: string;
  shapes: Array<{ slug: AnatomySlug; side: AnatomySide }>;
  markers?: AnatomyMarker[];
}

export const shapeKey = (view: AnatomyView, slug: AnatomySlug, side: AnatomySide) => `${view}:${slug}:${side}`;
