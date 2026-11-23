export interface Box { x: number; y: number; width: number; height: number }
export interface StageLayout {
  mode: 'row' | 'column';
  width: number;
  height: number;
  cloud: Box;
  lock: { x: number; y: number; r: number };
  card: Box;
}

export const TEMPLATE_H = 48;
const PAD = 8;
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

export const stageLayout = (width: number, height: number): StageLayout => {
  if (width >= 760) {
    const r = width >= 1100 ? 54 : 46;
    const cardW = Math.round(clamp(width * 0.27, 240, 340));
    const cardX = width - cardW - PAD;
    const cloudW = Math.round(Math.min(width * 0.46, cardX - 2 * r - 140));
    const cardTop = TEMPLATE_H;
    const cardH = Math.round(clamp(height - cardTop - PAD, 300, 460));
    const lockY = cardTop + cardH / 2;
    return {
      mode: 'row', width, height,
      cloud: { x: 0, y: PAD, width: cloudW, height: height - 2 * PAD },
      lock: { x: Math.round((cloudW + cardX) / 2), y: Math.round(lockY), r },
      card: { x: cardX, y: cardTop, width: cardW, height: cardH },
    };
  }
  const r = 44;
  const cloudH = Math.round(clamp(width * 0.95, 360, 480));
  const lockY = cloudH + 64;
  const cardW = Math.round(Math.min(width - 2 * PAD, 420));
  const cardTop = lockY + r + 110 + TEMPLATE_H;
  const cardH = 380;
  return {
    mode: 'column', width, height: cardTop + cardH + PAD,
    cloud: { x: 0, y: PAD, width, height: cloudH },
    lock: { x: Math.round(width / 2), y: lockY, r },
    card: { x: Math.round((width - cardW) / 2), y: cardTop, width: cardW, height: cardH },
  };
};
