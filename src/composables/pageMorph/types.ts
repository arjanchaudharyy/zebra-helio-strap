import type { Flight, FlightSnapshot } from '../../lib/motion/pageFlight';
import type { ScrollGuard } from '../../lib/motion/scrollGuard';
import type { Trail } from './links';

export interface Live {
  kind: 'expand' | 'collapse';
  page: HTMLElement;
  path: string;
  under: HTMLElement | null;
  flight: Flight;
  veil: HTMLElement | null;
  veilAnim: Animation | null;
  hidden: Animation | null;
  release: () => void;
  pageDone: () => void;
  underDone: () => void;
  stop: () => void;
  hurry: () => void;
  go?: () => void;
  landed: boolean;
  pageGone: boolean;
  underGone: boolean;
}
export interface Stale extends Live {
  snap: FlightSnapshot;
  veilNow: number;
  elapsed: number;
  turned: boolean;
  claimed: Set<Element>;
}

export type Collapse = {
  el: HTMLElement; trail: Trail; backdrop: HTMLElement; staying: HTMLElement | null; release: () => void;
  path: string;
  lift: number;
  done: () => void; stayingDone: () => void; frame: number;
};

export interface MorphState {
  live: Live | null;
  stale: Stale | null;
  pending: Map<Element, () => void>;
  entered: WeakSet<Element>;
  hold: (() => void) | null;
  guard: ScrollGuard | null;
  pendingCollapse: Collapse | null;
  expandLeaving: { el: HTMLElement; done: () => void } | null;
  expandPath: string;
  trails: Map<string, Trail>;
}

export const createState = (): MorphState => ({
  live: null, stale: null, pending: new Map(), entered: new WeakSet(), hold: null, guard: null,
  pendingCollapse: null, expandLeaving: null, expandPath: '', trails: new Map(),
});
