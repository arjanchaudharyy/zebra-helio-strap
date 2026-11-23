import { describe, expect, it } from 'vitest';
import { eventSpanLabel, overlapsEvent, validLifeEvent } from '../lifeEvents';
import type { LifeEventInput } from '../../types';

const event: LifeEventInput = { id: null, title: 'Resfriado', category: 'health',
  startDate: '2026-09-01', endDate: '2026-09-05', notes: '', epoch: false };

describe('life events calendar context', () => {
  it('includes either boundary and open ranges without timezone conversion', () => {
    expect(overlapsEvent(event, '2026-09-05', '2026-09-10')).toBe(true);
    expect(overlapsEvent(event, '2026-08-31', '2026-09-01')).toBe(true);
    expect(overlapsEvent(event, '2026-09-06', '2026-09-10')).toBe(false);
    expect(overlapsEvent({ ...event, endDate: null }, '2027-01-01', '2027-02-01')).toBe(true);
    expect(overlapsEvent(event, '2026-08-01', '2026-08-31')).toBe(false);
  });
  it('rejects impossible, reversed and non-calendar dates', () => {
    expect(validLifeEvent(event)).toBe(true);
    for (const startDate of ['2026-02-30', '2026-9-01', '2026-09-01T00:00:00Z']) {
      expect(validLifeEvent({ ...event, startDate })).toBe(false);
    }
    expect(validLifeEvent({ ...event, title: '  ' })).toBe(false);
    expect(validLifeEvent({ ...event, endDate: '2026-08-31' })).toBe(false);
  });
});

describe('eventSpanLabel', () => {
  it('single day, a span, and an ongoing event each read differently', () => {
    expect(eventSpanLabel({ ...event, endDate: event.startDate })).toBe('9/1');
    expect(eventSpanLabel(event)).toBe('9/1–9/5');
    expect(eventSpanLabel({ ...event, endDate: null })).toBe('from 9/1');
  });
});
