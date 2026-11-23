import { ref } from 'vue';
import lensCapL from '../assets/glass/lens-cap-l.png';
import lensCapR from '../assets/glass/lens-cap-r.png';
import lensMid from '../assets/glass/lens-mid.png';
import rimCapL from '../assets/glass/rim-cap-l.png';
import rimCapR from '../assets/glass/rim-cap-r.png';
import rimMid from '../assets/glass/rim-mid.png';

const SVG_NS = 'http://www.w3.org/2000/svg';
const FLAG_KEY = 'zeppbridge.glassLens';

const readFlag = (): boolean => {
  try {
    const fromQuery = new URLSearchParams(window.location.search).get('lens');
    if (fromQuery === '1' || fromQuery === '0') {
      window.localStorage.setItem(FLAG_KEY, fromQuery);
      return fromQuery === '1';
    }
    return window.localStorage.getItem(FLAG_KEY) !== '0';
  } catch {
    return false;
  }
};

export const lensSupported = (): boolean => {
  if (typeof window === 'undefined' || typeof CSS === 'undefined') return false;
  if (!/Chrome\//.test(navigator.userAgent)) return false;
  if (!CSS.supports('backdrop-filter', 'url(#zb) blur(2px)')) return false;
  return !window.matchMedia('(prefers-reduced-transparency: reduce)').matches;
};

export const lensEnabled = ref(typeof window !== 'undefined' && readFlag());

export type LensKind = 'thumb' | 'rim';

interface LensSpec {
  maps: { capL: string; mid: string; capR: string };
  strength: number;
  dispersion: number;
  blur: number;
  scatter: number;
}

const SPEC: Record<LensKind, LensSpec> = {
  thumb: { maps: { capL: lensCapL, mid: lensMid, capR: lensCapR }, strength: 0.135, dispersion: 0.1, blur: 0.024, scatter: 0.06 },
  rim: { maps: { capL: rimCapL, mid: rimMid, capR: rimCapR }, strength: 0.0675, dispersion: 0.08, blur: 0.02, scatter: 0.05 },
};

let host: SVGSVGElement | null = null;
let seq = 0;

const ensureHost = (): SVGSVGElement => {
  if (host?.isConnected) return host;
  host = document.createElementNS(SVG_NS, 'svg');
  host.setAttribute('aria-hidden', 'true');
  host.setAttribute('width', '0');
  host.setAttribute('height', '0');
  host.style.position = 'absolute';
  host.style.width = '0';
  host.style.height = '0';
  host.style.pointerEvents = 'none';
  document.body.appendChild(host);
  return host;
};

const node = <K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string>, children: SVGElement[] = []): SVGElementTagNameMap[K] => {
  const el = document.createElementNS(SVG_NS, tag);
  for (const [key, value] of Object.entries(attrs)) el.setAttribute(key, value);
  el.append(...children);
  return el;
};

const CHANNEL: Record<'R' | 'G' | 'B', string> = {
  R: '1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0',
  G: '0 0 0 0 0  0 1 0 0 0  0 0 0 0 0  0 0 0 1 0',
  B: '0 0 0 0 0  0 0 0 0 0  0 0 1 0 0  0 0 0 1 0',
};
const alphaOnly = (func: Record<string, string>) => node('feComponentTransfer', {}, [
  node('feFuncA', func),
]);

export interface LensFilter {
  ref: string;
  resize: (width: number, height: number) => void;
  dispose: () => void;
}

export const createLensFilter = (kind: LensKind): LensFilter => {
  const spec = SPEC[kind];
  const id = `zb-lens-${kind}-${(seq += 1)}`;
  const filter = node('filter', {
    id,
    filterUnits: 'userSpaceOnUse',
    primitiveUnits: 'userSpaceOnUse',
    'color-interpolation-filters': 'sRGB',
  });
  const neutral = node('feFlood', { 'flood-color': 'rgb(128,128,0)', 'flood-opacity': '0', result: 'neutral' });
  const capL = node('feImage', { href: spec.maps.capL, preserveAspectRatio: 'none', result: 'capL' });
  const mid = node('feImage', { href: spec.maps.mid, preserveAspectRatio: 'none', result: 'mid' });
  const capR = node('feImage', { href: spec.maps.capR, preserveAspectRatio: 'none', result: 'capR' });
  const map = node('feMerge', { result: 'map' }, ['neutral', 'mid', 'capL', 'capR'].map((input) => node('feMergeNode', { in: input })));

  const displacers = (['R', 'G', 'B'] as const).map((channel) =>
    node('feDisplacementMap', { in: 'SourceGraphic', in2: 'map', xChannelSelector: 'R', yChannelSelector: 'G', result: `d${channel}` }));
  const isolate = (['R', 'G', 'B'] as const).map((channel) =>
    node('feColorMatrix', { in: `d${channel}`, type: 'matrix', values: CHANNEL[channel], result: `c${channel}` }));
  const sumRG = node('feComposite', { in: 'cR', in2: 'cG', operator: 'arithmetic', k1: '0', k2: '1', k3: '1', k4: '0', result: 'rg' });
  const sharp = node('feComposite', { in: 'rg', in2: 'cB', operator: 'arithmetic', k1: '0', k2: '1', k3: '1', k4: '0', result: 'sharp' });

  const magnitude = node('feColorMatrix', { in: 'map', type: 'matrix', values: '0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 1 0 0', result: 'm' });
  const soft = node('feGaussianBlur', { in: 'sharp', result: 'soft' });
  const blurWeight = alphaOnly({ type: 'gamma', amplitude: '1', exponent: '1.6', offset: '0' });
  blurWeight.setAttribute('in', 'm');
  blurWeight.setAttribute('result', 'bw');
  const softIn = node('feComposite', { in: 'soft', in2: 'bw', operator: 'in', result: 'softIn' });
  const lensed = node('feMerge', { result: 'lensed' }, ['sharp', 'softIn'].map((input) => node('feMergeNode', { in: input })));
  const veilColor = node('feFlood', { 'flood-color': '#FFFFFF', result: 'white' });
  veilColor.style.setProperty('flood-color', 'var(--lens-scatter, #FFFFFF)');
  const veilWeight = alphaOnly({ type: 'linear', slope: String(spec.scatter), intercept: '0' });
  veilWeight.setAttribute('in', 'm');
  veilWeight.setAttribute('result', 'vw');
  const veil = node('feComposite', { in: 'white', in2: 'vw', operator: 'in', result: 'veil' });
  const glowed = node('feMerge', { result: 'glowed' }, ['lensed', 'veil'].map((input) => node('feMergeNode', { in: input })));
  const rimWeight = alphaOnly({ type: 'linear', slope: '12', intercept: '0' });
  rimWeight.setAttribute('in', 'm');
  rimWeight.setAttribute('result', 'rw');
  const rim = node('feComposite', { in: 'glowed', in2: 'rw', operator: 'in', result: 'rim' });
  const shaped = node('feComposite', { in: 'rim', in2: 'map', operator: 'in', result: 'shaped' });
  const out = node('feMerge', {}, ['SourceGraphic', 'shaped'].map((input) => node('feMergeNode', { in: input })));

  filter.append(neutral, capL, mid, capR, map, ...displacers, ...isolate, sumRG, sharp, magnitude, soft, blurWeight, softIn, lensed,
    veilColor, veilWeight, veil, glowed, rimWeight, rim, shaped, out);
  ensureHost().appendChild(filter);

  const place = (el: SVGElement, x: number, y: number, width: number, height: number) => {
    el.setAttribute('x', String(x));
    el.setAttribute('y', String(y));
    el.setAttribute('width', String(Math.max(0, width)));
    el.setAttribute('height', String(Math.max(0, height)));
  };

  let last = '';
  const resize = (width: number, height: number) => {
    const w = Math.max(1, Math.round(width));
    const h = Math.max(1, Math.round(height));
    const key = `${w}x${h}`;
    if (key === last) return;
    last = key;
    const r = Math.min(h / 2, w / 2);
    place(filter, 0, 0, w, h);
    place(neutral, 0, 0, w, h);
    place(capL, 0, 0, r, h);
    place(capR, w - r, 0, r, h);
    place(mid, r - 1, 0, w - 2 * r + 2, h);
    place(map, 0, 0, w, h);
    place(veilColor, 0, 0, w, h);
    const reach = h * spec.strength * 2;
    const [dR, dG, dB] = displacers;
    dR!.setAttribute('scale', (reach * (1 + spec.dispersion)).toFixed(2));
    dG!.setAttribute('scale', reach.toFixed(2));
    dB!.setAttribute('scale', (reach * (1 - spec.dispersion)).toFixed(2));
    soft.setAttribute('stdDeviation', Math.max(0.5, h * spec.blur).toFixed(2));
  };

  return { ref: `url(#${id})`, resize, dispose: () => filter.remove() };
};
