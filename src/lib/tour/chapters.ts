
export interface TourStep {
  target: string | null;
  route?: string;
  all?: boolean;
  interactive?: boolean;
  fallback?: { route: string; target: string };
  open?: 'pickDays';
}

export const INTRO = 'intro' as const;
export const SECTIONS = ['home', 'metric', 'workouts', 'sleep', 'ai', 'plan', 'journal', 'devices', 'settings'] as const;

export type SectionId = (typeof SECTIONS)[number];
export type TourChapterId = typeof INTRO | SectionId;
export const ALL_CHAPTERS: readonly TourChapterId[] = [INTRO, ...SECTIONS];

const openCard = (id: string) => `.deck-stage .open-card[data-deck-open="${id}"]`;

export const TOUR_STEPS: Record<TourChapterId, TourStep[]> = {
  intro: [
    { route: '/', target: '.theme-toggle', interactive: true },
    { route: '/settings/account', target: openCard('account') },
    { route: '/', target: '.pins' },
    { route: '/', target: '.pill-nav, .bottom-track' },
    { route: '/', target: '.sync-pill' },
    { route: '/settings/guide', target: openCard('guide') },
  ],
  home: [
    { route: '/', target: '.overview-focus', interactive: true },
    { route: '/', target: '.overview-page .dashboard-grid' },
    { route: '/', target: '.overview-page .feedback-card' },
    { route: '/', target: '.overview-page .coach-inbox' },
    { route: '/', target: '.overview-page .journal-entry' },
  ],
  metric: [
    { route: '/heart', target: '.range-bar', interactive: true },
    { route: '/heart', target: '[data-tour="metric-info"]' },
    { route: '/heart', target: '.pick-days' },
    { route: '/heart', target: '.card-table .table-layer:not(.pending) .pcard', all: true, open: 'pickDays', interactive: true },
  ],
  workouts: [
    { route: '/workouts', target: '.selectable-record' },
    { route: '/workouts', target: '.selection-actions' },
  ],
  sleep: [
    { route: '/sleep', target: '.list-page .record-row.tone-sleep' },
  ],
  ai: [
    { route: '/ai', target: '.ai-stage-page .cloud .slot', all: true, interactive: true },
    { route: '/ai', target: '.ai-stage-page .future' },
    { route: '/ai', target: '.ai-stage-page .bridge-lock' },
    { route: '/ai', target: '.ai-stage-page .mcp-pill' },
  ],
  plan: [
    { route: '/ai/plan', target: '.ai-plan-week .future-days' },
    { route: '/ai/plan', target: '.ai-plan-week .future-day' },
    { route: '/ai/plan', target: '.ai-plan-week .hold-button' },
    { route: '/ai/plan', target: '.ai-plan-week [data-tour="plan-history"]' },
  ],
  journal: [
    { route: '/journal', target: '.journal-page .year-dashboard' },
    { route: '/journal', target: '.journal-page .record-wall' },
    { route: '/journal', target: '.journal-page .tone-race' },
    { route: '/journal', target: '.journal-page .tone-shoe' },
    { route: '/rings', target: '.rings-page .tree-rings' },
  ],
  devices: [
    { route: '/settings/account', target: `${openCard('account')} [data-tour="devices"]` },
    { route: '/health-check', target: '.health-page .summary-card' },
  ],
  settings: [
    { route: '/settings', target: '.card-deck .deck-list' },
    { route: '/settings/archive', target: openCard('archive') },
    { route: '/settings/ai', target: openCard('ai') },
  ],
};

export const isSection = (id: string): id is SectionId => (SECTIONS as readonly string[]).includes(id);
export const isTourChapter = (id: string): id is TourChapterId => id in TOUR_STEPS;
