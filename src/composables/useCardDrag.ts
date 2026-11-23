import { nextTick, ref, shallowRef } from 'vue';
import { useCardCollection } from './useCardCollection';
import { reducedMotion, SPRINGS, springCurve } from '../lib/motion/cards';
import { centerOfRect } from '../lib/motion/cards/pose';

const START_PX = 6;
const BOX_SLOP = 14;
const PILE_STEP = { x: 5, y: -6 };

export interface Pile { anchor: string; cards: string[] }

export const useCardDrag = (options: {
  cardOf: (id: string) => HTMLElement | null;
  canDrag: (id: string) => boolean;
  canPileOn: (id: string) => boolean;
  isDay: (id: string) => boolean;
  onStart?: () => void;
  dropInBox: (id: string, el: HTMLElement) => void;
}) => {
  const box = useCardCollection();
  const piles = shallowRef<Pile[]>([]);
  const draggingId = ref<string | null>(null);
  const targetId = ref<string | null>(null);
  let drag: { id: string; el: HTMLElement; pointer: number; x0: number; y0: number; x: number; y: number; base: { x: number; y: number }; moved: boolean; frame: number } | null = null;
  let swallowClick = false;

  const boxRect = (): DOMRect | null => {
    const el = document.getElementById('card-collection-box');
    if (!el) return null;
    const rect = el.getBoundingClientRect();
    if (box.pointerDragging.value || el.classList.contains('is-docked')) {
      const height = Math.max(rect.height, 70);
      return new DOMRect(window.innerWidth - 220, window.innerHeight - height - 16, 220, height);
    }
    return rect;
  };
  const pileOf = (id: string) => piles.value.find((pile) => pile.cards.includes(id)) ?? null;
  const topOf = (pile: Pile) => pile.cards[pile.cards.length - 1]!;
  const isPileTop = (id: string) => { const pile = pileOf(id); return !!pile && topOf(pile) === id; };
  const pileCount = (id: string) => { const pile = pileOf(id); return pile && topOf(pile) === id ? pile.cards.length : 0; };
  const buried = (id: string) => { const pile = pileOf(id); return !!pile && topOf(pile) !== id; };

  const parseTranslate = (el: HTMLElement) => {
    const m = /(-?[\d.]+)px(?:\s+(-?[\d.]+)px)?/.exec(el.style.translate || '');
    return m ? { x: Number(m[1]), y: Number(m[2] ?? 0) } : { x: 0, y: 0 };
  };
  const slotCenter = (el: HTMLElement) => centerOfRect((el.parentElement ?? el).getBoundingClientRect());

  const layoutPiles = (animate = true) => {
    const { easing, duration } = springCurve(SPRINGS.settle);
    for (const pile of piles.value) {
      const anchor = options.cardOf(pile.anchor);
      if (!anchor) continue;
      const at = slotCenter(anchor);
      pile.cards.forEach((id, i) => {
        const el = options.cardOf(id);
        if (!el || id === draggingId.value) return;
        const own = slotCenter(el);
        const to = { x: at.x - own.x + PILE_STEP.x * i, y: at.y - own.y + PILE_STEP.y * i };
        const tilt = i === 0 ? 0 : ((i * 7) % 5) - 2;
        place(el, to, tilt, i + 10, animate ? { easing, duration } : null);
      });
    }
  };
  const place = (el: HTMLElement, to: { x: number; y: number }, tilt: number, z: number | null, spring: { easing: string; duration: number } | null) => {
    const from = parseTranslate(el);
    const fromRotate = el.style.rotate || '0deg';
    el.style.translate = to.x || to.y ? `${to.x.toFixed(1)}px ${to.y.toFixed(1)}px` : '';
    el.style.rotate = tilt ? `${tilt}deg` : '';
    if (el.parentElement) el.parentElement.style.zIndex = z === null ? '' : String(z);
    if (spring && !reducedMotion() && (Math.abs(from.x - to.x) > 0.5 || Math.abs(from.y - to.y) > 0.5)) {
      el.animate([{ translate: `${from.x}px ${from.y}px`, rotate: fromRotate }, { translate: `${to.x}px ${to.y}px`, rotate: `${tilt}deg` }], spring);
    }
  };

  const unpile = (ids: string[], animate = true) => {
    const gone = new Set(ids);
    const next: Pile[] = [];
    const freed: string[] = [];
    for (const pile of piles.value) {
      const cards = pile.cards.filter((id) => !gone.has(id));
      if (cards.length >= 2) next.push({ anchor: cards[0]!, cards });
      else freed.push(...cards);
    }
    piles.value = next;
    const { easing, duration } = springCurve(SPRINGS.settle);
    for (const id of [...freed, ...ids]) {
      const el = options.cardOf(id);
      if (el && id !== draggingId.value) place(el, { x: 0, y: 0 }, 0, null, animate ? { easing, duration } : null);
    }
    layoutPiles(animate);
  };

  let rects: Array<{ el: HTMLElement; rect: DOMRect }> = [];
  const snapRects = (dragged: HTMLElement) => {
    const layer = dragged.closest('.table-layer');
    rects = [...(layer?.querySelectorAll<HTMLElement>('.pcard[data-card-id]') ?? [])].filter((el) => el !== dragged).map((el) => ({ el, rect: el.getBoundingClientRect() }));
  };
  const cardAt = (x: number, y: number): HTMLElement | null | undefined => {
    let best: HTMLElement | null = null;
    let bestD = Infinity;
    for (const { el, rect } of rects) {
      if (!el.isConnected) return undefined;
      if (x < rect.left || x > rect.right || y < rect.top || y > rect.bottom) continue;
      const d = Math.hypot(x - (rect.left + rect.width / 2), y - (rect.top + rect.height / 2));
      if (d < bestD) { bestD = d; best = el; }
    }
    return best;
  };
  const pileTarget = (card: HTMLElement): { kind: 'card'; id: string } | null => {
    const id = card.dataset.cardId!;
    const pile = pileOf(id);
    const top = pile ? topOf(pile) : id;
    return top !== drag?.id && options.canPileOn(top) ? { kind: 'card', id: top } : null;
  };

  const hit = (x: number, y: number): { kind: 'box' } | { kind: 'card'; id: string } | null => {
    const r = boxRect();
    if (r && x >= r.left - BOX_SLOP && x <= r.right + BOX_SLOP && y >= r.top - BOX_SLOP && y <= r.bottom + BOX_SLOP) return { kind: 'box' };
    if (!drag || !options.isDay(drag.id)) return null;
    const cached = cardAt(x, y);
    if (cached !== undefined) return cached ? pileTarget(cached) : null;
    for (const el of document.elementsFromPoint(x, y)) {
      const card = el.closest<HTMLElement>('.pcard[data-card-id]');
      if (!card || card === drag.el) continue;
      return pileTarget(card);
    }
    return null;
  };

  const paint = () => {
    if (!drag) return;
    drag.frame = 0;
    const over = hit(drag.x, drag.y);
    const dx = drag.base.x + drag.x - drag.x0;
    const dy = drag.base.y + drag.y - drag.y0;
    drag.el.style.translate = `${dx.toFixed(1)}px ${dy.toFixed(1)}px`;
    box.dropHover.value = over?.kind === 'box';
    targetId.value = over?.kind === 'card' ? over.id : null;
  };

  const down = (event: PointerEvent, id: string) => {
    if (event.button !== 0 || drag) return;
    swallowClick = false;
    if (buried(id) || !options.canDrag(id)) return;
    const el = event.currentTarget as HTMLElement;
    drag = { id, el, pointer: event.pointerId, x0: event.clientX, y0: event.clientY, x: event.clientX, y: event.clientY, base: parseTranslate(el), moved: false, frame: 0 };
    window.addEventListener('pointerup', up, true);
    window.addEventListener('pointercancel', cancelEvent, true);
  };
  const move = (event: PointerEvent) => {
    if (!drag || event.pointerId !== drag.pointer) return;
    drag.x = event.clientX;
    drag.y = event.clientY;
    if (!drag.moved) {
      if (Math.hypot(drag.x - drag.x0, drag.y - drag.y0) < START_PX) return;
      drag.moved = true;
      draggingId.value = drag.id;
      box.pointerDragging.value = true;
      drag.el.setPointerCapture(event.pointerId);
      drag.el.classList.add('dragging');
      if (drag.el.parentElement) drag.el.parentElement.style.zIndex = '60';
      if (options.isDay(drag.id)) snapRects(drag.el);
      options.onStart?.();
    }
    if (!drag.frame) drag.frame = requestAnimationFrame(paint);
  };

  const finish = async (drop: boolean) => {
    const d = drag;
    if (!d) return;
    drag = null;
    rects = [];
    window.removeEventListener('pointerup', up, true);
    window.removeEventListener('pointercancel', cancelEvent, true);
    cancelAnimationFrame(d.frame);
    d.el.classList.remove('dragging');
    box.dropHover.value = false;
    const target = targetId.value;
    targetId.value = null;
    if (!d.moved) {
      box.pointerDragging.value = false;
      return;
    }
    swallowClick = true;
    const over = drop ? hit(d.x, d.y) : null;
    box.pointerDragging.value = false;
    draggingId.value = null;
    if (over?.kind === 'box') {
      unpile([d.id], false);
      d.el.style.translate = `${(d.base.x + d.x - d.x0).toFixed(1)}px ${(d.base.y + d.y - d.y0).toFixed(1)}px`;
      options.dropInBox(d.id, d.el);
      await nextTick();
      d.el.style.translate = '';
      d.el.style.rotate = '';
      if (d.el.parentElement) d.el.parentElement.style.zIndex = '';
      return;
    }
    if (over?.kind === 'card' || (drop && target)) {
      const onto = over?.kind === 'card' ? over.id : target!;
      const from = pileOf(d.id);
      if (from) unpile([d.id], true);
      const existing = pileOf(onto);
      piles.value = existing
        ? piles.value.map((pile) => (pile === existing ? { ...pile, cards: [...pile.cards, d.id] } : pile))
        : [...piles.value, { anchor: onto, cards: [onto, d.id] }];
      layoutPiles(true);
      return;
    }
    if (pileOf(d.id)) unpile([d.id], true);
    else {
      const { easing, duration } = springCurve(SPRINGS.settle);
      place(d.el, { x: 0, y: 0 }, 0, null, { easing, duration });
    }
  };
  const up = (event: PointerEvent) => { if (drag && event.pointerId === drag.pointer) void finish(true); };
  const cancelEvent = (event: PointerEvent) => { if (drag && event.pointerId === drag.pointer) void finish(false); };

  const cancel = (): boolean => {
    if (!drag?.moved) return false;
    void finish(false);
    return true;
  };
  const consumeClick = (): boolean => { const was = swallowClick; swallowClick = false; return was; };
  const reset = () => { piles.value = []; draggingId.value = null; targetId.value = null; drag = null; box.dropHover.value = false; box.pointerDragging.value = false; };

  return { piles, draggingId, targetId, down, move, up, cancelEvent, cancel, consumeClick, reset, unpile, layoutPiles, pileOf, isPileTop, pileCount, buried };
};
