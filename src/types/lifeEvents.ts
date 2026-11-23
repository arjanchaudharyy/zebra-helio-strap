
export interface LifeEventInput {
  id: number | null;
  title: string;
  category: 'health' | 'travel' | 'routine' | 'training' | 'other';
  startDate: string;
  endDate: string | null;
  notes: string;
  epoch: boolean;
}

export interface LifeEvent extends LifeEventInput {
  id: number;
  createdAt: string;
  updatedAt: string;
}
