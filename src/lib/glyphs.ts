import type { IconName } from '../components/Icon.vue';
import type { DesignIconName } from '../components/DesignIcon.vue';

export type GlyphTone =
  | 'neutral'
  | 'accent'
  | 'heart'
  | 'sleep'
  | 'activity'
  | 'training'
  | 'pace'
  | 'calories'
  | 'altitude';

export interface GlyphSpec {
  glyph?: IconName;
  tone: GlyphTone;
  image?: boolean;
  bare?: boolean;
}

const GLYPHS: Record<DesignIconName, GlyphSpec> = {
  'ai-ready': { glyph: 'spark', tone: 'accent' },
  'app-icon': { tone: 'accent', image: true },
  'auto-sync': { glyph: 'clock', tone: 'training' },
  'body-activity': { glyph: 'activity', tone: 'activity' },
  'brand-mark': { tone: 'accent', image: true },
  'browser-login': { glyph: 'globe', tone: 'neutral' },
  'chevron-right': { glyph: 'chevron-right', tone: 'neutral', bare: true },
  'cloud-output': { glyph: 'export', tone: 'accent' },
  database: { glyph: 'database', tone: 'neutral' },
  document: { glyph: 'file', tone: 'neutral' },
  handoff: { glyph: 'send', tone: 'accent' },
  'health-watch': { glyph: 'mountain', tone: 'altitude' },
  'heart-rate': { glyph: 'heart', tone: 'heart' },
  maintenance: { glyph: 'wrench', tone: 'neutral' },
  'manual-entry': { glyph: 'edit', tone: 'neutral' },
  'outdoor-cycling': { glyph: 'bike', tone: 'activity' },
  'outdoor-run': { glyph: 'run', tone: 'activity' },
  overview: { glyph: 'grid', tone: 'neutral' },
  pace: { glyph: 'pace', tone: 'pace' },
  power: { glyph: 'bolt', tone: 'training' },
  stride: { glyph: 'stride', tone: 'activity' },
  contact: { glyph: 'contact', tone: 'activity' },
  oscillation: { glyph: 'oscillation', tone: 'activity' },
  ratio: { glyph: 'ratio', tone: 'activity' },
  private: { glyph: 'lock', tone: 'accent' },
  profile: { glyph: 'user', tone: 'neutral' },
  recovery: { glyph: 'battery-heart', tone: 'heart' },
  'resting-heart-rate': { glyph: 'heart-rest', tone: 'heart' },
  secure: { glyph: 'shield', tone: 'accent' },
  settings: { glyph: 'gear', tone: 'neutral' },
  sleep: { glyph: 'moon', tone: 'sleep' },
  'sleep-waves': { glyph: 'moon', tone: 'sleep' },
  steps: { glyph: 'steps', tone: 'activity' },
  'structured-data': { glyph: 'braces', tone: 'neutral' },
  'training-load': { glyph: 'gauge', tone: 'training' },
  verified: { glyph: 'circle-check', tone: 'accent' },
  'vo2-max': { glyph: 'vo2', tone: 'training' },
  'zepp-cloud': { tone: 'accent', image: true },
};

export const glyphFor = (name: DesignIconName): GlyphSpec => GLYPHS[name];
