<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef } from 'vue';
import Icon from '../Icon.vue';
import SegmentTrack from '../SegmentTrack.vue';
import PlayingCard from './PlayingCard.vue';
import { useCardsText } from './cards.i18n';
import { useCardCollection } from '../../composables/useCardCollection';
import { useCardPicking } from '../../composables/useCardPicking';
import { useCardTableDrag } from '../../composables/useCardTableDrag';
import {
  animateFromNow, CARD_RETURN_MS, dealCards, fanBack, receive, recedeLayer, reducedMotion, relay, returnStack, settled, stackAndReturnMs, stackCards, type Landing, type Receded,
} from '../../lib/motion/cards';
import { OPEN_EASE, paced } from '../../lib/motion/timing';
import { onMotionEscape } from '../../lib/motion/interrupt';
import { cardIdOf, DECK_RANGES, expandGroup, focusWindow, rangeStart, rootLevel, type DeckDay, type DeckGroup, type DeckLevel, type DeckRange } from '../../lib/cards/deck';
import { deckToday, type DeckSource } from '../../lib/cards/sources';
import type { CoverageContext } from '../../lib/cards/coverage';
import { useGroupViews } from '../../composables/useGroupViews';
import { whenFramesSteady } from '../../lib/motion/steady';
import { fadeSourceIn, holdSource, sourceInFrames } from '../../lib/motion/sourceHold';
import { backend } from '../../lib/bridge';
import { useSyncController } from '../../composables/useSyncController';
import { useCardSpan } from '../../composables/useCardSpan';
import { tableLayerStyle, tableSlotStyle } from '../../lib/cards/tableLayout';
import { displayDateTimeFormatter, parseDisplayDate } from '../../lib/dateTime';

const props = withDefaults(defineProps<{ source: DeckSource; range: DeckRange; origin?: DOMRect | null; anchor?: Element | null; focus?: string | null }>(), { origin: null, anchor: null, focus: null });
const emit = defineEmits<{ close: [] }>();
const t = useCardsText();
const box = useCardCollection();
const PARK_FADE_MS = paced(300);

interface Entry { id: string; level: DeckLevel; title: string; receded: (Receded & { ready: Promise<void> }) | null; pending: boolean }
const range = ref<DeckRange>(props.focus ? 7 : props.range);
const stack = shallowRef<Entry[]>([]);
const layers: HTMLElement[] = [];

const stage = ref<HTMLElement | null>(null);
const backdrop = ref<HTMLElement | null>(null);
const head = ref<HTMLElement | null>(null);
const foot = ref<HTMLElement | null>(null);
const stageBox = ref({ width: 1000, height: 560 });
const busy = ref(false);
const closing = ref(false);
const loadFailed = ref(false);
const focusDate = ref<string | null>(props.focus);
const titleId = `card-table-${Math.random().toString(36).slice(2, 8)}`;
const seq = relay();
let returnFocus: HTMLElement | null = null;
let entrySeq = 0;
let popping = false;
let boxOpen = false;

const setLayer = (depth: number, el: unknown) => { if (el instanceof HTMLElement) layers[depth] = el; };
const top = computed(() => stack.value[stack.value.length - 1] ?? null);
const cardsOf = (depth: number): HTMLElement[] => [...(layers[depth]?.querySelectorAll<HTMLElement>('.pcard') ?? [])];
const cardOf = (id: string) => layers[stack.value.length - 1]?.querySelector<HTMLElement>(`[data-card-id="${CSS.escape(id)}"]`) ?? null;
const deep = ref<HTMLElement | null>(null);
let deepRun: Animation | null = null;
const deepen = () => {
  const el = deep.value;
  if (!el || reducedMotion()) return;
  el.hidden = false;
  const run = animateFromNow(el, [{ opacity: 1, offset: 0.35 }, { opacity: 0 }], { duration: 760, easing: 'ease-in-out' }, ['opacity']);
  deepRun = run;
  void run.finished.then(() => { if (deepRun === run) el.hidden = true; }, () => undefined);
};
const picking = useCardPicking({ source: () => props.source, busy, cardOf, settle: () => seq.settle(), onFly: deepen });

const { drag, scatter, hintId, onDayDown, onDayMove, onDayUp, onDayCancel, onDayTap } = useCardTableDrag({
  level: () => top.value?.level ?? null, cardOf, picking, blocked: () => busy.value || closing.value, has: (date) => box.has(props.source.key, date),
});

const { on: spanOn, from: spanFrom, tap: spanTap, toggle: toggleSpan, reset: resetSpan } = useCardSpan({
  level: () => top.value?.level ?? null, picking, blocked: () => busy.value || closing.value,
});
const tapDay = (day: DeckDay, event: MouseEvent) => {
  if (drag.consumeClick() || picking.consumeHold()) return;
  if (spanTap(event, cardIdOf(day))) return;
  onDayTap(day, event);
};

const { appStatus } = useSyncController();
const coverageCtx = shallowRef<CoverageContext | null>(null);
const loadCoverage = async () => {
  let ledger = null;
  try { ledger = await backend.getCoverageLedger(); } catch { ledger = null; }
  coverageCtx.value = { category: props.source.category, earliestLocal: appStatus.value?.coverage?.earliest_day ?? null, ledger };
};

const md = (date: string) => displayDateTimeFormatter({ month: 'numeric', day: 'numeric' }).format(parseDisplayDate(date));
const weekday = (date: string) => displayDateTimeFormatter({ weekday: 'short' }).format(parseDisplayDate(date));
const monthName = (date: string) => displayDateTimeFormatter({ month: 'long' }).format(parseDisplayDate(date));
const yearMonth = (date: string) => displayDateTimeFormatter({ year: 'numeric', month: 'short' }).format(parseDisplayDate(date));
const numberText = (value: number | null) => (value === null || !props.source.format ? null : props.source.format(value));
const dayValue = (day: DeckDay) => day.text ?? numberText(day.value) ?? (day.has ? t.value.recorded : null);
const workouts = computed(() => props.source.kind === 'workouts');
const dayAriaValue = (day: DeckDay) => { const n = numberText(day.value); return n ? `${n}${props.source.unit ? ` ${props.source.unit}` : ''}` : day.has ? t.value.recorded : t.value.noRecord; };
const dayAria = (day: DeckDay) => {
  const state = picking.stateOf(cardIdOf(day));
  if (state === 'boxed') return t.value.boxedAria(md(day.date));
  if (state === 'confirm') return t.value.confirmAria(md(day.date));
  return t.value.dayAria(md(day.date), day.text ?? dayAriaValue(day), false);
};
const { views: groupViews, groupTitle } = useGroupViews({
  levels: () => stack.value.map((entry) => entry.level), source: () => props.source, coverage: () => coverageCtx.value, numberText, md, monthName, yearMonth,
});
const gv = (group: DeckGroup) => groupViews.value.get(group.id)!;
const rangeLabel = (value: DeckRange) => ({ 7: t.value.range7, 30: t.value.range30, 180: t.value.range180, 365: t.value.range365, 3650: t.value.rangeAll })[value];
const rangeItems = computed(() => DECK_RANGES.map((value) => ({ value: String(value), label: rangeLabel(value) })));
const crumbs = computed(() => stack.value.map((entry) => entry.title).join(' › '));
const hint = computed(() => (top.value?.level.kind === 'groups' ? t.value.hintGroups : t.value.tipDays));
const pickedCount = computed(() => box.picks.value.filter((pick) => pick.key === props.source.key).length);

const layerStyle = (level: DeckLevel) => tableLayerStyle(level, stageBox.value);
const slotStyle = (level: DeckLevel, i: number) => (reducedMotion() ? {} : tableSlotStyle(level, i, stageBox.value));
const raisedAt = computed(() => {
  const level = top.value?.level;
  return picking.raisedId.value && level?.kind === 'days' ? level.days.findIndex((day) => cardIdOf(day) === picking.raisedId.value) : -1;
});
const slotClass = (depth: number, i: number) => {
  const at = raisedAt.value;
  if (depth !== stack.value.length - 1 || at < 0 || at === i) return null;
  return i < at ? 'shift-l' : 'shift-r';
};

const anchorLanding = (): Landing | null => {
  const rect = props.anchor?.isConnected ? props.anchor.getBoundingClientRect() : props.origin;
  return rect ? { rect, kind: 'button' } : null;
};
let releaseAnchor: () => void = () => undefined;
const holdAnchor = () => {
  releaseAnchor();
  releaseAnchor = !reducedMotion() && props.anchor instanceof HTMLElement && anchorLanding() ? holdSource(props.anchor) : () => undefined;
};
const stageCenter = () => {
  const r = stage.value?.getBoundingClientRect();
  return r ? { x: r.left + r.width / 2, y: r.top + r.height / 2 } : undefined;
};
const firstPickable = (cards: HTMLElement[]) => cards.find((card) => card.classList.contains('focused'))
  ?? cards.find((card) => card.getAttribute('aria-disabled') !== 'true');
const pulse = (card: HTMLElement | undefined) => {
  if (!card || reducedMotion()) return;
  card.querySelector('.pcard-glow')?.animate([{ opacity: 0 }, { opacity: 1, offset: 0.35 }, { opacity: 0 }], { duration: 600, easing: 'ease-out' });
};

const loadLevel = async (): Promise<DeckLevel | null> => {
  try {
    loadFailed.value = false;
    const today = deckToday();
    if (focusDate.value) {
      const [start, end] = focusWindow(focusDate.value, today);
      return { kind: 'days', days: await props.source.loadDays(start, end) };
    }
    return rootLevel(await props.source.loadDays(rangeStart(today, range.value), today), range.value);
  } catch {
    loadFailed.value = true;
    return null;
  }
};
const rootTitle = () => (focusDate.value ? t.value.aroundDay(md(focusDate.value)) : rangeLabel(range.value));

const dealSteadily = async (cards: HTMLElement[], from: Landing | null, fromStack = false) => {
  const before = new Set(cards.flatMap((card) => card.getAnimations()));
  const dealt = dealCards(cards, from, { fromStack, track: seq.track });
  const fresh = cards.flatMap((card) => card.getAnimations()).filter((animation) => !before.has(animation));
  for (const animation of fresh) animation.pause();
  await whenFramesSteady();
  for (const animation of fresh) if (animation.playState === 'paused') animation.play();
  await dealt;
};

const deal = async (id: number) => {
  const level = await loadLevel();
  if (!level || !seq.live(id)) return;
  layers.length = 0;
  stack.value = [{ id: `e${entrySeq++}`, level, title: rootTitle(), receded: null, pending: false }];
  await nextTick();
  const cards = cardsOf(0);
  holdAnchor();
  await dealSteadily(cards, anchorLanding());
  if (!seq.live(id)) return;
  const first = firstPickable(cards);
  first?.focus({ preventScroll: true });
  if (focusDate.value) pulse(cards.find((card) => card.classList.contains('focused')));
};

const push = async (group: DeckGroup, card: HTMLElement) => {
  if (busy.value || closing.value) return;
  const id = seq.handoff();
  scatter();
  resetSpan();
  busy.value = true;
  await picking.cancelConfirm();
  const depth = stack.value.length - 1;
  const entry: Entry = { id: `e${entrySeq++}`, level: expandGroup(group), title: groupTitle(group), receded: null, pending: true };
  const receded = recedeLayer(layers[depth]!, card, () => seq.live(id));
  const below = { ...stack.value[depth]!, receded };
  stack.value = [...stack.value.slice(0, depth), below, entry];
  await receded.ready;
  if (!seq.live(id)) return;
  stack.value = [...stack.value.slice(0, depth + 1), { ...entry, pending: false }];
  await nextTick();
  const cards = cardsOf(depth + 1);
  busy.value = false;
  await dealSteadily(cards, receded.now());
  if (!seq.live(id)) return;
  firstPickable(cards)?.focus({ preventScroll: true });
};

const pop = async () => {
  if (popping || closing.value) return;
  const depth = stack.value.length - 1;
  if (depth <= 0) { await close(); return; }
  const id = seq.handoff();
  scatter();
  resetSpan();
  popping = true;
  busy.value = true;
  await picking.cancelConfirm();
  const leaving = stack.value[depth]!;
  const below = stack.value[depth - 1]!;
  const cards = leaving.pending ? [] : cardsOf(depth);
  if (cards.length) await stackCards(cards);
  if (seq.live(id)) await Promise.all([returnStack(cards, below.receded?.pile ?? null), below.receded?.restore()]);
  popping = false;
  if (!seq.live(id)) return;
  layers.length = depth;
  stack.value = [...stack.value.slice(0, depth - 1), { ...below, receded: null }];
  await nextTick();
  busy.value = false;
  firstPickable(cardsOf(depth - 1))?.focus({ preventScroll: true });
};

const CHROME_OUT_EASE = 'cubic-bezier(.45, 0, .4, 1)';
const fadeChrome = (to: number, duration: number) => {
  const easing = to ? OPEN_EASE : CHROME_OUT_EASE;
  const under = layers.slice(0, -1).filter((_, i) => !stack.value[i]?.receded);
  const els = [backdrop.value, head.value, foot.value, ...under].filter((el): el is HTMLElement => !!el);
  return els.map((el) => animateFromNow(el, { opacity: to }, { duration, easing, fill: 'forwards' }, ['opacity']));
};
const setBoxOpen = (open: boolean) => {
  if (boxOpen === open) return;
  boxOpen = open;
  box.setTableOpen(open);
};

const close = async () => {
  if (closing.value) return;
  const id = seq.handoff();
  scatter();
  closing.value = true;
  busy.value = true;
  picking.holdEnd();
  const cards = cardsOf(stack.value.length - 1).filter((card) => card.style.visibility !== 'hidden');
  const receiver = receive(props.anchor);
  const fadeMs = reducedMotion() ? 160 : stackAndReturnMs(cards.length);
  const fades = fadeChrome(0, fadeMs);
  const parked = stack.value.map((entry) => entry.receded?.fadeOut(fadeMs));
  await stackCards(cards, stageCenter());
  if (!seq.live(id)) { receiver.cancel(); return; }
  setBoxOpen(false);
  const landing = anchorLanding();
  if (landing && props.anchor instanceof HTMLElement) fadeSourceIn(props.anchor, sourceInFrames(0.6, 0.95), { duration: CARD_RETURN_MS, easing: 'linear' });
  await Promise.all([returnStack(cards, landing), ...fades.map((a) => a.finished.catch(() => undefined)), ...parked]);
  if (!seq.live(id)) { receiver.cancel(); return; }
  releaseAnchor();
  receiver.land(props.source.tint);
  for (const entry of stack.value) entry.receded?.drop();
  emit('close');
  returnFocus?.focus({ preventScroll: true });
};

const reopen = () => {
  if (!closing.value) return;
  seq.next();
  closing.value = false;
  busy.value = false;
  props.anchor?.classList.remove('is-receiving');
  holdAnchor();
  setBoxOpen(true);
  fadeChrome(1, 360);
  for (const entry of stack.value) void entry.receded?.fadeIn(360);
  void fanBack(cardsOf(stack.value.length - 1));
};

const switchRange = async (value: string) => {
  const next = Number(value) as DeckRange;
  if ((next === range.value && !focusDate.value) || closing.value) return;
  const id = seq.handoff();
  scatter();
  resetSpan();
  range.value = next;
  focusDate.value = null;
  busy.value = true;
  const loading = loadLevel();
  await picking.cancelConfirm();
  if (!seq.live(id)) return;
  const shown = stack.value.filter((entry) => !entry.pending);
  if (shown.length !== stack.value.length) { stack.value = shown; await nextTick(); }
  const depth = stack.value.length - 1;
  const old = cardsOf(depth);
  const parked = stack.value.slice(0, depth).map((entry, i) => entry.receded?.fadeOut(PARK_FADE_MS)
    ?? animateFromNow(layers[i]!, { opacity: 0 }, { duration: PARK_FADE_MS, fill: 'forwards' }, ['opacity']).finished.then(() => undefined, () => undefined));
  const center = stageCenter();
  const stacked = stackCards(old, center);
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  const level = await loading;
  if (!seq.live(id)) return;
  if (!level) { busy.value = false; return; }
  const entry: Entry = { id: `e${entrySeq++}`, level, title: rootTitle(), receded: null, pending: true };
  stack.value = [...stack.value, entry];
  await Promise.all([stacked.then((animations) => settled(animations)), nextTick()]);
  if (!seq.live(id)) return;
  const topCard = old[old.length - 1];
  const width = (topCard?.offsetWidth ?? 120);
  const at = center ?? { x: window.innerWidth / 2, y: window.innerHeight / 2 };
  const from: Landing = { rect: new DOMRect(at.x - width / 2, at.y - width * 0.7, width, width * 1.4), kind: 'pile', angle: 0, width };
  await Promise.all(parked);
  if (!seq.live(id)) return;
  for (const below of stack.value) below.receded?.drop();
  layers.length = 0;
  stack.value = [{ ...entry, pending: false }];
  await nextTick();
  const cards = cardsOf(0);
  busy.value = false;
  await dealSteadily(cards, from, true);
  if (!seq.live(id)) return;
  firstPickable(cards)?.focus({ preventScroll: true });
};

const nearCards = (event: MouseEvent) => cardsOf(stack.value.length - 1).some((card) => {
  const r = card.getBoundingClientRect();
  return event.clientX > r.left - 28 && event.clientX < r.right + 28 && event.clientY > r.top - 28 && event.clientY < r.bottom + 28;
});
const onBlank = (event: MouseEvent) => {
  if (picking.consumeHold() || drag.consumeClick()) return;
  if (picking.confirmId.value) { void picking.cancelConfirm(); return; }
  if (nearCards(event)) return;
  void pop();
};
const tapGroup = (group: DeckGroup, event: MouseEvent) => {
  if (drag.consumeClick() || picking.consumeHold()) return;
  if (spanTap(event, group.id)) return;
  void push(group, event.currentTarget as HTMLElement);
};
const onCardKey = (event: KeyboardEvent, item: { group?: DeckGroup }) => {
  const level = top.value?.level;
  if (event.key === 'Enter' && event.shiftKey && level) {
    event.preventDefault();
    void picking.pickAll(level.kind === 'days' ? level.days : null, level.kind === 'groups' ? level.groups : null);
    return;
  }
  if (item.group && event.key === ' ') { event.preventDefault(); void picking.pickGroup(item.group, event.currentTarget as HTMLElement); }
};

const introHead = () => {
  if (reducedMotion()) return;
  [...(head.value?.querySelectorAll<HTMLElement>('[data-intro]') ?? [])].forEach((el, i) => el.animate(
    [{ opacity: 0, transform: 'translateY(-6px)' }, { opacity: 1, transform: 'none' }],
    { duration: 360, delay: 60 + i * 50, easing: OPEN_EASE, fill: 'backwards' },
  ));
};

let releaseEscape: (() => void) | null = null;
let resize: ResizeObserver | null = null;
const measure = () => {
  const el = stage.value;
  if (el) stageBox.value = { width: el.clientWidth - 20, height: el.clientHeight - 24 };
  if (drag.piles.value.length) void nextTick(() => drag.layoutPiles(false));
};
onMounted(() => {
  returnFocus = document.activeElement as HTMLElement | null;
  releaseEscape = onMotionEscape(() => {
    if (drag.cancel()) return true;
    if (picking.confirmId.value) { void picking.cancelConfirm(); return true; }
    if (!closing.value) void pop();
    return true;
  });
  measure();
  resize = new ResizeObserver(measure);
  if (stage.value) resize.observe(stage.value);
  setBoxOpen(true);
  backdrop.value?.animate([{ opacity: 0 }, { opacity: 1 }], { duration: reducedMotion() ? 140 : 420, easing: OPEN_EASE, fill: 'backwards' });
  introHead();
  void deal(seq.next());
  if (!workouts.value) void loadCoverage();
});
onBeforeUnmount(() => {
  releaseAnchor();
  releaseEscape?.();
  resize?.disconnect();
  picking.holdEnd();
  setBoxOpen(false);
  for (const entry of stack.value) entry.receded?.drop();
});
defineExpose({ reopen, isClosing: () => closing.value });
</script>

<template>
  <Teleport to="body">
    <div :class="['card-table', { closing }]" role="dialog" aria-modal="true" :aria-labelledby="titleId" :aria-describedby="`${titleId}-keys`" :style="{ '--tint': source.tint }">
      <div ref="backdrop" class="table-backdrop" aria-hidden="true" @click="onBlank"></div>
      <div ref="deep" class="table-backdrop deep" aria-hidden="true" hidden></div>
      <header ref="head" class="table-head">
        <button v-if="stack.length > 1" type="button" class="table-icon" :aria-label="t.back" @click="pop"><Icon name="arrow-left" :size="17" /></button>
        <span v-else class="table-mark" data-intro aria-hidden="true"><Icon name="cards" :size="18" /></span>
        <div class="table-titles" data-intro>
          <h2 :id="titleId">{{ source.label }}</h2>
          <p>{{ t.pickSubtitle }} · {{ crumbs }}<template v-if="pickedCount"> · <b>{{ t.pickedHere(pickedCount) }}</b></template><template v-if="spanOn"> · <b>{{ spanFrom ? t.spanFrom : t.spanAria }}</b></template></p>
        </div>
        <span data-intro><SegmentTrack compact intro :items="rangeItems" :model-value="String(range)" :aria-label="t.rangeAria" @update:model-value="switchRange" /></span>
        <button type="button" :class="['table-icon', 'span-toggle', { on: spanOn }]" data-intro :aria-pressed="spanOn" :aria-label="t.spanAria" :title="t.spanAria" @click="toggleSpan"><Icon name="link" :size="16" /><span>{{ t.span }}</span></button>
        <button type="button" class="table-icon" data-intro data-table-close :aria-label="t.close" @click="close"><Icon name="x" :size="17" /></button>
      </header>
      <div ref="stage" class="table-stage" @click.self="onBlank">
        <p v-if="loadFailed || (top && top.level.kind === 'days' && !top.level.days.length)" class="table-empty">{{ workouts ? t.noWorkouts : t.noRecord }}</p>
        <template v-for="(entry, depth) in stack" :key="entry.id">
          <div :ref="(el) => setLayer(depth, el)" :class="['table-layer', { pending: entry.pending }]" :style="layerStyle(entry.level)" @click.self="onBlank">
            <template v-if="entry.level.kind === 'days'">
              <span v-for="(day, i) in entry.level.days" :key="cardIdOf(day)" :class="['pslot', slotClass(depth, i)]" :style="slotStyle(entry.level, i)">
                <PlayingCard kind="day" :data-card-id="cardIdOf(day)" :top="weekday(day.date)" :title="md(day.date)" :corner="String(Number(day.date.slice(8)))"
                  :value="source.second ? numberText(day.value) : dayValue(day)" :unit="!day.text && numberText(day.value) ? source.unit : null"
                  :value2="source.second && day.value2 != null ? source.second.format(day.value2) : null" :unit2="source.second?.unit ?? null" :tint2="source.second?.tint ?? null" :icon="day.icon" :state="day.has ? picking.stateOf(cardIdOf(day)) : 'front'"
                  :class="{ buried: drag.buried(cardIdOf(day)), 'span-from': spanOn && spanFrom === cardIdOf(day) }"
                  :disabled="!day.has" :focused="day.date === focusDate && (!workouts || i === entry.level.days.findIndex((d) => d.date === focusDate))" :tint="source.tint"
                  :badge="drag.pileCount(cardIdOf(day)) ? (hintId === cardIdOf(day) ? t.holdPile : `×${drag.pileCount(cardIdOf(day))}`) : null"
                  :aria-label="dayAria(day)" :aria-pressed="picking.stateOf(cardIdOf(day)) === 'boxed'"
                  @pointerdown="onDayDown($event, day)" @pointermove="onDayMove" @pointerup="onDayUp" @pointercancel="onDayCancel"
                  @click="tapDay(day, $event)" @keydown="onCardKey($event, {})" />
              </span>
            </template>
            <template v-else>
              <span v-for="(group, i) in entry.level.groups" :key="group.id" class="pslot" :style="slotStyle(entry.level, i)">
                <PlayingCard kind="group" :deck="group.kind" :picked="gv(group).picked" :data-card-id="group.id" :top="gv(group).top" :title="gv(group).title" :value="gv(group).value"
                  :dots="gv(group).dots" :empty="gv(group).empty" :coverage="gv(group).coverage" :class="{ 'span-from': spanOn && spanFrom === group.id }" :badge="gv(group).badge" :tint="source.tint"
                  :aria-label="gv(group).aria" :aria-description="gv(group).description"
                  @pointerdown="picking.holdStart(group, $event); drag.down($event, group.id)" @pointermove="picking.holdMove($event); drag.move($event)"
                  @pointerup="picking.holdEnd(); drag.up($event)" @pointercancel="picking.holdEnd(); drag.cancelEvent($event)"
                  @click="tapGroup(group, $event)" @keydown="onCardKey($event, { group })" />
              </span>
            </template>
          </div>
        </template>
      </div>
      <footer ref="foot" class="table-foot">
          <p :id="`${titleId}-keys`" class="sr-only">{{ hint }} {{ t.keyboardTip }}</p>
      </footer>
    </div>
  </Teleport>
</template>

<style scoped src="./CardTable.css"></style>
