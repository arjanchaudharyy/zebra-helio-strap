import { describe, expect, it } from 'vitest';
import { anchoredLeft, popoverStyle, resolveDropUp, VIEWPORT_MARGIN } from '../popoverPosition';

describe('anchoredLeft: opens toward the free side', () => {
  it('a button near the left opens to the right and stays off the left edge', () => {
    expect(anchoredLeft({ left: 40, right: 70 }, 340, 1280, 16)).toBe(40);
    expect(anchoredLeft({ left: 4, right: 34 }, 340, 1280, 16)).toBe(16);
  });

  it('opens to the left only when the right has no room (right edges align)', () => {
    expect(anchoredLeft({ left: 1200, right: 1230 }, 340, 1280, 16)).toBe(1230 - 340);
  });

  it('centers and clamps into the window when neither side has room', () => {
    const left = anchoredLeft({ left: 180, right: 210 }, 360, 400, 16);
    expect(left).toBeGreaterThanOrEqual(16);
    expect(left + 360).toBeLessThanOrEqual(400 - 16);
  });
});


const viewport = { width: 1280, height: 800 };
const sizing = { maxHeight: 300, width: 240 };

describe('popover positioning', () => {
  it('drops down when there is room below', () => {
    const trigger = { top: 100, bottom: 130, left: 400, width: 120 };
    expect(resolveDropUp(trigger, viewport, sizing)).toBe(false);
    const style = popoverStyle(trigger, viewport, sizing);
    expect(style.top).toBe('134px');
    expect(style.bottom).toBeUndefined();
  });

  it('flips up when the trigger sits near the bottom edge', () => {
    const trigger = { top: 740, bottom: 770, left: 400, width: 120 };
    expect(resolveDropUp(trigger, viewport, sizing)).toBe(true);
    const style = popoverStyle(trigger, viewport, sizing);
    expect(style.bottom).toBe(`${800 - 740 + 4}px`);
    expect(style.top).toBeUndefined();
  });

  it('picks the roomier side and shrinks to fit when neither side is generous', () => {
    const short = { width: 1280, height: 400 };
    const trigger = { top: 250, bottom: 280, left: 400, width: 120 };
    expect(resolveDropUp(trigger, short, sizing)).toBe(true);
    const style = popoverStyle(trigger, short, sizing);
    expect(Number.parseInt(style.maxHeight, 10)).toBeLessThanOrEqual(250 - VIEWPORT_MARGIN);
  });

  it('never lets a wide popover run off the right edge', () => {
    const trigger = { top: 100, bottom: 130, left: 1200, width: 60 };
    const style = popoverStyle(trigger, viewport, sizing);
    const left = Number.parseInt(style.left, 10);
    expect(left + 240).toBeLessThanOrEqual(viewport.width - VIEWPORT_MARGIN);
  });

  it('positions a language menu wider than its trigger within a narrow window', () => {
    const width = 224;
    const style = popoverStyle(
      { top: 70, bottom: 105, left: 390, width: 84 },
      { width: 480, height: 640 },
      { maxHeight: 268, width },
    );
    expect(style.width).toBe(`${width}px`);
    expect(Number.parseInt(style.left, 10) + width).toBeLessThanOrEqual(480 - VIEWPORT_MARGIN);
  });

  it('never lets it run off the left edge either', () => {
    const trigger = { top: 100, bottom: 130, left: -40, width: 60 };
    const style = popoverStyle(trigger, viewport, sizing);
    expect(Number.parseInt(style.left, 10)).toBeGreaterThanOrEqual(VIEWPORT_MARGIN);
  });

  it('keeps a usable height even in a very short window', () => {
    const tiny = { width: 520, height: 320 };
    const trigger = { top: 150, bottom: 180, left: 200, width: 120 };
    const style = popoverStyle(trigger, tiny, sizing);
    expect(Number.parseInt(style.maxHeight, 10)).toBeGreaterThan(0);
  });
});
