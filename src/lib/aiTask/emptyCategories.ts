import type { AiTaskCategory } from '../bridge/types';
import type { DayStripRow } from '../../types/timeBridge';

export const emptyCategories = (rows: DayStripRow[]): Set<AiTaskCategory> =>
  new Set(rows.filter((row) => row.cells.length > 0 && row.cells.every((cell) => !cell.has && cell.value === null)).map((row) => row.category));
