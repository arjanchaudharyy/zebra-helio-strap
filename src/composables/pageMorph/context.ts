import type { Ledger } from './ledger';
import type { MorphState } from './types';

export interface MorphCtx {
  state: MorphState;
  ledger: Ledger;
  back: () => void;
}
