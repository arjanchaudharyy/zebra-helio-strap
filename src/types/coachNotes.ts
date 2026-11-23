export type CoachNoteState = 'proposed' | 'accepted' | 'rejected';
export type CoachNoteSource = 'user' | 'mcp';

export interface CoachNote {
  id: number;
  text: string;
  state: CoachNoteState;
  source: CoachNoteSource;
  source_name: string | null;
  created_at: string;
  updated_at: string;
  decided_at: string | null;
}

export interface CoachNotes {
  accepted: CoachNote[];
  proposed: CoachNote[];
}
