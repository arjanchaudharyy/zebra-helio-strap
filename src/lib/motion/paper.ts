import { reducedMotion } from './cards';
import { paced } from './timing';

const EASE = 'cubic-bezier(.4, .6, .2, 1)';
const W = 260;
const H = 150;
const MARGIN = 12;

type Rect = { x: number; y: number; w: number; h: number };
const rectOf = (box: DOMRect): Rect => ({ x: box.left, y: box.top, w: box.width, h: box.height });
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const wait = (ms: number) => new Promise<void>((resolve) => window.setTimeout(resolve, ms));
const settled = async (el: HTMLElement) => {
  let last = '';
  for (let i = 0; i < 36; i += 1) {
    await frame();
    const r = el.getBoundingClientRect();
    const now = `${Math.round(r.left)},${Math.round(r.top)},${Math.round(r.width)},${Math.round(r.height)}`;
    if (now === last) return;
    last = now;
  }
};

export interface Paper {
  pullOut(): Promise<void>;
  fill(lines: string[], caption: string): void;
  fail(reason: string): Promise<void>;
  land(host: HTMLElement | null, commit: () => Promise<boolean>, reason: () => string): Promise<boolean>;
  dispose(): void;
}

export const createPaper = (button: HTMLElement): Paper => {
  const wrap = document.createElement('div');
  wrap.className = 'zb-paper';
  wrap.setAttribute('aria-hidden', 'true');
  const sheet = document.createElement('div');
  sheet.className = 'zb-paper-sheet';
  const text = document.createElement('div');
  text.className = 'zb-paper-text';
  sheet.append(text);
  wrap.append(sheet);
  wrap.style.width = `${W}px`;
  wrap.style.height = `${H}px`;
  wrap.style.opacity = '0';
  document.body.append(wrap);
  let box = { w: W, h: H };

  const pose = (r: Rect) => `translate(${r.x.toFixed(1)}px, ${r.y.toFixed(1)}px) scale(${(r.w / box.w).toFixed(4)}, ${(r.h / box.h).toFixed(4)})`;
  const go = async (el: HTMLElement, to: Keyframe, duration: number, easing = EASE) => {
    const from = getComputedStyle(el);
    const start: Keyframe = { transform: from.transform === 'none' ? 'none' : from.transform, opacity: from.opacity };
    const anim = el.animate([start, to], { duration: reducedMotion() ? 1 : paced(duration), easing, fill: 'forwards' });
    await anim.finished.catch(() => undefined);
    try { anim.commitStyles(); } catch { }
    anim.cancel();
  };
  const buttonRect = () => rectOf(button.getBoundingClientRect());
  const outRect = (): Rect => {
    const b = buttonRect();
    const x = Math.min(window.innerWidth - W - MARGIN, Math.max(MARGIN, b.x + b.w / 2 - W / 2));
    const y = b.y - H - 10 >= MARGIN ? b.y - H - 10 : Math.min(window.innerHeight - H - MARGIN, b.y + b.h + 10);
    return { x, y, w: W, h: H };
  };

  const pullOut = async () => {
    wrap.style.transform = pose(buttonRect());
    await frame();
    await go(wrap, { transform: pose(outRect()), opacity: '1' }, 500);
  };

  const fill = (lines: string[], caption: string) => {
    text.replaceChildren(...lines.map((line) => {
      const row = document.createElement('span');
      row.textContent = line;
      return row;
    }));
    const small = document.createElement('small');
    small.textContent = caption;
    text.append(small);
  };

  const back = async () => {
    await go(wrap, { transform: pose(buttonRect()), opacity: '0' }, 440, 'cubic-bezier(.4, 0, .7, .2)');
    dispose();
  };

  const fail = async (reason: string) => {
    fill([], reason);
    wrap.classList.add('is-failed');
    await go(text, { opacity: '1' }, 1);
    await wait(reducedMotion() ? 600 : 1100);
    await back();
  };

  const land = async (host: HTMLElement | null, commit: () => Promise<boolean>, reason: () => string): Promise<boolean> => {
    if (!host || !host.isConnected) {
      const ok = await commit();
      if (!ok) { await fail(reason()); return false; }
      await go(wrap, { transform: `${pose(outRect())} translateY(-14px)`, opacity: '0' }, 360);
      dispose();
      return true;
    }
    const target = rectOf(host.getBoundingClientRect());
    const old = [...host.querySelectorAll<HTMLElement>('[data-paper-row]')];
    void go(text, { opacity: '0' }, 200);
    const retreat = old.map((row, i) => row.animate([{ opacity: 1, transform: 'none' }, { opacity: 0, transform: 'translateY(8px)' }],
      { duration: paced(200), delay: paced(180 + i * 30), easing: 'cubic-bezier(.4, 0, 1, 1)', fill: 'forwards' }));
    await go(wrap, { transform: pose(target), opacity: '1' }, 500);
    box = { w: target.w, h: target.h };
    wrap.style.width = `${target.w}px`;
    wrap.style.height = `${target.h}px`;
    wrap.style.transform = pose(target);
    host.setAttribute('data-paper-hold', '');
    for (const anim of retreat) anim.finish();

    const ok = await commit();
    if (!ok) {
      host.removeAttribute('data-paper-hold');
      for (const anim of retreat) anim.reverse();
      box = { w: W, h: H };
      wrap.style.width = `${W}px`;
      wrap.style.height = `${H}px`;
      wrap.style.transform = pose(target);
      await fail(reason());
      for (const anim of retreat) anim.cancel();
      return false;
    }
    for (const anim of retreat) anim.cancel();
    await frame();
    await settled(host);
    const now = rectOf(host.getBoundingClientRect());
    box = { w: now.w, h: now.h };
    wrap.style.width = `${now.w}px`;
    wrap.style.height = `${now.h}px`;
    wrap.style.transform = pose(now);
    const rows = [...host.querySelectorAll<HTMLElement>('[data-paper-row]')];
    const across = rows.length > 1 && rows[1]!.getBoundingClientRect().left >= rows[0]!.getBoundingClientRect().right - 1;
    const MOVE = reducedMotion() ? 1 : paced(170);
    const SHOW = reducedMotion() ? 1 : paced(150);
    const step = MOVE + SHOW;
    const edges = rows.map((row) => {
      const r = row.getBoundingClientRect();
      return across ? r.right - now.x : r.bottom - now.y;
    });
    const total = rows.length * step + MOVE;
    const offsets: Keyframe[] = [{ transform: 'translate(0, 0)', offset: 0 }];
    edges.forEach((edge, i) => {
      const at = (i * step) / total;
      const done = (i * step + MOVE) / total;
      const prev = i === 0 ? 0 : edges[i - 1]!;
      offsets.push({ transform: across ? `translate(${prev}px, 0)` : `translate(0, ${prev}px)`, offset: at, easing: EASE });
      offsets.push({ transform: across ? `translate(${edge}px, 0)` : `translate(0, ${edge}px)`, offset: done });
    });
    offsets.push({ transform: across ? `translate(${now.w + 4}px, 0)` : `translate(0, ${now.h + 4}px)`, offset: 1 });
    const slide = sheet.animate(offsets, { duration: total, easing: 'linear', fill: 'forwards' });
    host.removeAttribute('data-paper-hold');
    rows.forEach((row, i) => {
      row.animate([{ opacity: 0, transform: across ? 'translateX(-6px)' : 'translateY(-6px)' }, { opacity: 1, transform: 'none' }],
        { duration: SHOW + 120, delay: i * step + MOVE, easing: EASE, fill: 'backwards' });
    });
    await slide.finished.catch(() => undefined);
    dispose();
    return true;
  };

  const dispose = () => wrap.remove();

  return { pullOut, fill, fail, land, dispose };
};
