import type { TrainingPlanState } from '../../types/trainingPlan';

export type PlanStatusKind = 'sending' | 'draft' | 'delivered' | 'partial' | 'unconfirmed' | 'rejected';

export const planStatus = (state: TrainingPlanState | null, drafting: boolean, sending = false): PlanStatusKind | null => {
  if (sending) return 'sending';
  if (drafting) return 'draft';
  const record = state?.last_publish;
  if (!record) return null;
  if (record.state === 'rejected') return 'rejected';
  if (record.state === 'partial') return 'partial';
  if (record.state === 'sent' && !state?.uncertain) return 'delivered';
  return 'unconfirmed';
};

export const statusTone = (kind: PlanStatusKind): 'ok' | 'warn' | 'bad' | 'draft' | 'sending' => {
  switch (kind) {
    case 'delivered': return 'ok';
    case 'rejected': return 'bad';
    case 'draft': return 'draft';
    case 'sending': return 'sending';
    default: return 'warn';
  }
};
