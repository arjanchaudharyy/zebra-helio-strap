import { dialogWindowFrames } from './dialogFlight';
import { exemptFromSettle } from './interrupt';
import { lessMotion } from './lessMotion';
import { cardReplica } from './replica';
import { settleCssAnimations } from './settle';
import { whenFramesSteady } from './steady';
import { OPEN_EASE, paced } from './timing';
import { visibleWindow, type WindowRect } from './window';

export const SHEET_MS = paced(480);
const NEARLY_HIDDEN = 0.001;
const SHADOW_MS = paced(140);
const BLEED = 90;

export const sheetSource = (name: string): HTMLElement | null => {
  const el = document.querySelector<HTMLElement>(`[data-sheet="${CSS.escape(name)}"]`);
  return el && el.getClientRects().length ? el : null;
};

export const sheetRadius = (source: WindowRect, sourceRadius: number): number =>
  Math.max(0, Math.min(sourceRadius || source.height / 2, source.height / 2, source.width / 2, 24));

export const sheetClip = (radius: number) => ({
  flush: `inset(0px round ${radius}px)`,
  bleed: `inset(${-BLEED}px round ${radius + BLEED}px)`,
});

export const SHEET_BODY_IN: Keyframe[] = [{ opacity: NEARLY_HIDDEN }, { opacity: NEARLY_HIDDEN, offset: 0.45 }, { opacity: 1 }];
export const SHEET_REPLICA_OUT: Keyframe[] = [{ opacity: 1 }, { opacity: 0, offset: 0.3 }, { opacity: 0 }];
export const SHEET_REPLICA_IN: Keyframe[] = [{ opacity: 0 }, { opacity: 0, offset: 0.4 }, { opacity: 1, offset: 0.8 }, { opacity: 1 }];
export const sheetVeilOut = (from: number): Keyframe[] => [{ opacity: from }, { opacity: from * 0.88, offset: 0.4 }, { opacity: 0 }];

const radiusOf = (el: HTMLElement) => parseFloat(getComputedStyle(el).borderTopLeftRadius) || 0;
const rectOf = (el: Element): WindowRect => {
  const box = el.getBoundingClientRect();
  return { left: box.left, top: box.top, width: box.width, height: box.height };
};

const hideSource = (el: HTMLElement | null) => el?.setAttribute('data-sheet-source', '');
const showSource = (name: string) => {
  document.querySelectorAll(`[data-sheet="${CSS.escape(name)}"][data-sheet-source]`).forEach((el) => el.removeAttribute('data-sheet-source'));
};

const attachReplica = (panel: HTMLElement, source: HTMLElement | null, frames: Keyframe[], duration: number) => {
  if (!source?.isConnected) return null;
  const replica = cardReplica(source).el;
  replica.querySelectorAll('[data-sheet-source]').forEach((node) => node.removeAttribute('data-sheet-source'));
  replica.dataset.sheetReplica = '';
  Object.assign(replica.style, { position: 'absolute', left: '0', top: '0', zIndex: '4', pointerEvents: 'none' });
  panel.appendChild(replica);
  settleCssAnimations(replica);
  replica.animate(frames, { duration, fill: 'both', easing: 'linear' });
  return replica;
};

let firstOpen = true;

export const openSheet = (root: HTMLElement, name: string): void => {
  const panel = root.querySelector<HTMLElement>('.sheet-panel');
  const body = root.querySelector<HTMLElement>('.sheet-body');
  const veil = root.querySelector<HTMLElement>('.sheet-veil');
  if (!panel || lessMotion()) return;
  const final = rectOf(panel);
  const source = sheetSource(name);
  veil?.animate([{ opacity: NEARLY_HIDDEN }, { opacity: 1 }], { duration: SHEET_MS * 0.7, easing: 'ease-out', fill: 'backwards' });
  if (!source) {
    panel.animate([{ transform: 'scale(.96)', opacity: 0 }, { transform: 'none', opacity: 1 }], { duration: paced(220), easing: OPEN_EASE, fill: 'backwards' });
    return;
  }
  const from = rectOf(source);
  const radius = sheetRadius(from, radiusOf(source));
  const replica = attachReplica(panel, source, SHEET_REPLICA_OUT, SHEET_MS);
  hideSource(source);
  const flight = panel.animate(dialogWindowFrames(from, final, final, radius), { duration: SHEET_MS, easing: OPEN_EASE, fill: 'both' });
  body?.animate(SHEET_BODY_IN, { duration: SHEET_MS, easing: 'ease-out', fill: 'both' });
  if (firstOpen) {
    firstOpen = false;
    const held = panel.getAnimations({ subtree: true });
    held.forEach((animation) => animation.pause());
    void whenFramesSteady(160).then(() => held.forEach((animation) => { if (animation.playState === 'paused') animation.play(); }));
  }
  const settle = () => {
    replica?.remove();
    body?.getAnimations().forEach((animation) => animation.cancel());
    const clip = sheetClip(radiusOf(panel) || 28);
    const shadow = panel.animate([{ clipPath: clip.flush }, { clipPath: clip.bleed }], { duration: SHADOW_MS, easing: 'ease-out' });
    flight.cancel();
    const clear = () => shadow.cancel();
    shadow.finished.then(clear, clear);
  };
  flight.finished.then(() => { if (root.isConnected) settle(); }, () => {});
};

export const leaveSheet = (el: Element, done: () => void) => {
  const root = el as HTMLElement;
  const name = root.dataset.sheetName ?? '';
  const panel = root.querySelector<HTMLElement>('.sheet-panel');
  const body = root.querySelector<HTMLElement>('.sheet-body');
  const veil = root.querySelector<HTMLElement>('.sheet-veil');
  if (!panel || lessMotion()) { showSource(name); done(); return; }
  root.style.pointerEvents = 'none';
  const style = getComputedStyle(panel);
  const now = { transform: style.transform, clipPath: style.clipPath };
  const seen = visibleWindow(panel);
  const bodyNow = body ? Number(getComputedStyle(body).opacity) : 1;
  const veilNow = veil ? Number(getComputedStyle(veil).opacity) : 1;
  for (const animation of root.getAnimations({ subtree: true })) animation.cancel();
  panel.querySelectorAll('[data-sheet-replica]').forEach((node) => node.remove());
  const final = rectOf(panel);
  const source = sheetSource(name);
  const timing = { duration: SHEET_MS, easing: OPEN_EASE, fill: 'forwards' as const };
  if (veil) exemptFromSettle(veil.animate(sheetVeilOut(veilNow), { ...timing, easing: 'linear' }));
  if (body) exemptFromSettle(body.animate([{ opacity: bodyNow }, { opacity: 0, offset: 0.3 }, { opacity: 0 }], { ...timing, easing: 'linear' }));
  let flight: Animation;
  if (source) {
    const to = rectOf(source);
    const frames = dialogWindowFrames(seen, to, final, sheetRadius(to, radiusOf(source)));
    frames[0] = { ...frames[0], transform: now.transform, ...(now.clipPath === 'none' ? {} : { clipPath: now.clipPath }) };
    attachReplica(panel, source, SHEET_REPLICA_IN, SHEET_MS);
    flight = exemptFromSettle(panel.animate(frames, timing));
  } else {
    flight = exemptFromSettle(panel.animate([{ transform: now.transform, opacity: 1 }, { transform: 'scale(.96)', opacity: 0 }], { ...timing, duration: paced(220) }));
  }
  const end = () => { showSource(name); done(); };
  flight.finished.then(end, end);
};
