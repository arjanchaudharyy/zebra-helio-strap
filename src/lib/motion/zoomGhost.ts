import { ghostBox, ZOOM_SETTLE_MS } from '../uiZoom';

const FREEZE_STYLE_ID = 'zb-zoom-ghost-style';
const FREEZE_CSS = `[data-zoom-ghost], [data-zoom-ghost] *, [data-zoom-ghost] *::before, [data-zoom-ghost] *::after {
  animation-delay: -60s !important; animation-play-state: paused !important; transition: none !important; caret-color: transparent !important;
}`;

const ensureFreezeStyle = () => {
  if (document.getElementById(FREEZE_STYLE_ID)) return;
  const style = document.createElement('style');
  style.id = FREEZE_STYLE_ID;
  style.textContent = FREEZE_CSS;
  document.head.appendChild(style);
};

const animatedStyles = (source: Element): Array<[string, string]> => {
  const animations = source.getAnimations?.() ?? [];
  if (!animations.length) return [];
  const computed = getComputedStyle(source);
  const props = new Set<string>();
  for (const animation of animations) {
    const effect = animation.effect as KeyframeEffect | null;
    if (!effect || effect.pseudoElement || effect.target !== source) continue;
    for (const frame of effect.getKeyframes()) {
      for (const key of Object.keys(frame)) {
        if (key === 'offset' || key === 'easing' || key === 'composite' || key === 'computedOffset') continue;
        props.add(key.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`));
      }
    }
  }
  return [...props].map((prop) => [prop, computed.getPropertyValue(prop)] as [string, string]).filter(([, value]) => value !== '');
};

export interface ZoomGhost {
  reveal(): void;
  dispose(): void;
}

export const captureZoomGhost = (ratio: number): ZoomGhost | null => {
  const body = document.body;
  if (!body) return null;
  const clone = body.cloneNode(true) as HTMLElement;
  const sources = body.getElementsByTagName('*');
  const copies = clone.getElementsByTagName('*');
  const scrolls: Array<[HTMLElement, number, number]> = [];
  if (body.scrollTop || body.scrollLeft) scrolls.push([clone, body.scrollTop, body.scrollLeft]);
  const count = Math.min(sources.length, copies.length);
  for (let i = 0; i < count; i += 1) {
    const source = sources[i] as HTMLElement;
    const copy = copies[i] as HTMLElement;
    if (source.scrollTop || source.scrollLeft) scrolls.push([copy, source.scrollTop, source.scrollLeft]);
    if (source instanceof HTMLCanvasElement && copy instanceof HTMLCanvasElement) {
      try { copy.getContext('2d')?.drawImage(source, 0, 0); } catch { }
    } else if (source instanceof HTMLInputElement || source instanceof HTMLTextAreaElement) {
      (copy as HTMLInputElement).value = source.value;
    }
    for (const [prop, value] of animatedStyles(source)) copy.style.setProperty(prop, value, 'important');
  }
  for (const el of Array.from(clone.querySelectorAll('iframe, video, audio, script'))) el.remove();

  const box = ghostBox(ratio);
  clone.removeAttribute('id');
  clone.setAttribute('data-zoom-ghost', '');
  clone.setAttribute('aria-hidden', 'true');
  clone.inert = true;
  Object.assign(clone.style, {
    position: 'fixed',
    top: '0',
    left: '50%',
    right: 'auto',
    bottom: 'auto',
    width: box.width,
    height: box.height,
    minWidth: '0',
    margin: '0',
    marginLeft: box.marginLeft,
    transform: 'none',
    zIndex: '2147483647',
    pointerEvents: 'none',
    overflow: 'hidden',
  } satisfies Partial<CSSStyleDeclaration>);

  let fade: Animation | null = null;
  const dispose = () => {
    fade?.cancel();
    fade = null;
    clone.remove();
  };
  return {
    reveal() {
      ensureFreezeStyle();
      document.documentElement.appendChild(clone);
      for (const [el, top, left] of scrolls) {
        el.scrollTop = top;
        el.scrollLeft = left;
      }
      fade = clone.animate([{ opacity: 1 }, { opacity: 0 }], { duration: ZOOM_SETTLE_MS, easing: 'ease-out', fill: 'forwards' });
      fade.finished.then(() => clone.remove(), () => undefined);
    },
    dispose,
  };
};
