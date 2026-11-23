import { backend } from './bridge';
import type { Query } from './readCache';
import type { Injury, PersonalRecords, Race, ShoeOverview } from '../types/logbook';
import type { AnnualRings } from '../types/rings';

export const journalQuery = {
  rings: (): Query<AnnualRings> => ({ key: 'annual_rings', fetch: () => backend.annualRings() }),
  records: (): Query<PersonalRecords> => ({ key: 'personal_records', fetch: () => backend.personalRecords() }),
  shoes: (): Query<ShoeOverview> => ({ key: 'shoes_overview', fetch: () => backend.shoesOverview() }),
  races: (): Query<Race[]> => ({ key: 'races_list', fetch: () => backend.racesList() }),
  injuries: (): Query<Injury[]> => ({ key: 'injuries_list', fetch: () => backend.injuriesList() }),
};

export const journalPageQueries = () => [
  journalQuery.rings(),
  journalQuery.records(),
  journalQuery.shoes(),
  journalQuery.races(),
  journalQuery.injuries(),
] as const;
