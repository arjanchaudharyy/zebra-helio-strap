<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import Icon, { type IconName } from '../../Icon.vue';
import SegmentTrack, { type SegmentItem } from '../../SegmentTrack.vue';
import LayoutGlyph, { type GlyphCell } from './LayoutGlyph.vue';
import { useMessages } from '../../../i18n';
import { stageMessages } from './stage.i18n';

type Act = 'solo' | 'full' | 'exitFull' | 'tuck';
const props = defineProps<{
  title: string;
  x: number;
  y: number;
  from: string;
  count: number;
  snap: number;
  exact: boolean;
  full: boolean;
}>();
const emit = defineEmits<{
  act: [kind: Act];
  snap: [snap: number];
  close: [];
}>();
const t = useMessages(stageMessages);
const root = ref<HTMLElement | null>(null);

const third = 1 / 3;
const GLYPHS: Record<number, readonly (readonly GlyphCell[])[]> = {
  1: [[[0.17, 0, 0.66, 1]], [[0.08, 0, 0.84, 1]]],
  2: [
    [[0, 0, 0.5, 1], [0.5, 0, 0.5, 1]],
    [[0, 0, 2 * third, 1], [2 * third, 0, third, 1]],
    [[0, 0, third, 1], [third, 0, 2 * third, 1]],
  ],
  3: [
    [[0, 0, 2 * third, 1], [2 * third, 0, third, 0.5], [2 * third, 0.5, third, 0.5]],
    [[0, 0, third, 0.5], [0, 0.5, third, 0.5], [third, 0, 2 * third, 1]],
    [[0, 0, 1, 0.5], [0, 0.5, 0.5, 0.5], [0.5, 0.5, 0.5, 0.5]],
  ],
  4: [
    [[0, 0, 0.5, 0.5], [0.5, 0, 0.5, 0.5], [0, 0.5, 0.5, 0.5], [0.5, 0.5, 0.5, 0.5]],
    [[0, 0, 1, 0.5], [0, 0.5, third, 0.5], [third, 0.5, third, 0.5], [2 * third, 0.5, third, 0.5]],
    [[0, 0, 0.6, 1], [0.6, 0, 0.4, third], [0.6, third, 0.4, third], [0.6, 2 * third, 0.4, third]],
  ],
};
const sizes = computed<(SegmentItem<number> & { cells: readonly GlyphCell[] })[]>(() => {
  const m = t.value;
  const names = props.count === 1 ? [m.sizeStandard, m.sizeWide]
    : props.count === 2 ? ['1 : 1', '2 : 1', '1 : 2']
      : props.count === 4 ? [m.grid, m.oneOverThree, m.bigLeft]
        : [m.mainLeft, m.mainRight, m.mainTop];
  const glyphs = GLYPHS[Math.min(Math.max(props.count, 1), 4)]!;
  return names.map((label, value) => ({ value, label, cells: glyphs[value]! }));
});
const cellsOf = (value: number) => sizes.value[value]?.cells ?? [];
const chosen = ref(props.exact ? props.snap : -1);
watch(() => [props.snap, props.exact] as const, ([snap, exact]) => { chosen.value = exact ? snap : -1; });
const pick = (index: number) => {
  if (chosen.value === index) return;
  chosen.value = index;
  emit('snap', index);
};
const actions = computed<{ kind: Act; label: string; icon: IconName }[]>(() => {
  const m = t.value;
  const rows: { kind: Act; label: string; icon: IconName }[] = [];
  if (props.count > 1 && !props.full) rows.push({ kind: 'solo', label: m.solo, icon: 'cards' });
  rows.push(props.full ? { kind: 'exitFull', label: m.exitFull, icon: 'undo' } : { kind: 'full', label: m.full, icon: 'monitor' });
  rows.push({ kind: 'tuck', label: m.tuck, icon: 'arrow-left' });
  return rows;
});

onMounted(() => { void nextTick(() => root.value?.querySelector<HTMLElement>('button')?.focus({ preventScroll: true })); });

const step = (event: KeyboardEvent) => {
  const rows = Array.from(root.value?.querySelectorAll<HTMLElement>('.menu-row') ?? []);
  const at = rows.findIndex((row) => row.contains(document.activeElement));
  const next = rows[(at + (event.key === 'ArrowDown' ? 1 : -1) + rows.length) % rows.length];
  (next?.matches('button') ? next : next?.querySelector<HTMLElement>('button[tabindex="0"], button'))?.focus();
};
</script>

<template>
  <div
    ref="root"
    class="stage-menu glass-control is-lens-host"
    role="menu"
    :aria-label="t.menu(title)"
    :style="{ left: `${x}px`, top: `${y}px` }"
    :data-from="from"
    @keydown.esc.stop.prevent="emit('close')"
    @keydown.down.prevent="step"
    @keydown.up.prevent="step"
  >
    <div class="menu-inner">
      <div v-if="!full" class="menu-row menu-seg">
        <SegmentTrack
          compact
          fill
          icon-only
          :items="sizes"
          :model-value="chosen"
          :placeholder="snap"
          :aria-label="count === 1 ? t.size : t.split"
          @update:model-value="pick"
        >
          <template #default="{ item }"><LayoutGlyph :cells="cellsOf(item.value)" /></template>
        </SegmentTrack>
      </div>
      <div v-if="!full" class="menu-sep" aria-hidden="true"></div>
      <button
        v-for="row in actions"
        :key="row.kind"
        type="button"
        role="menuitem"
        class="menu-row menu-action"
        @click="emit('act', row.kind)"
      >
        <Icon :name="row.icon" :size="16" />
        <span>{{ row.label }}</span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.stage-menu.glass-control {
  position: absolute;
  z-index: 1000;
  width: 320px;
  border-radius: 22px;
  box-shadow: none;
}
.stage-menu.glass-control.is-lens-host::before { box-shadow: var(--glass-rim), var(--glass-shadow); }
.menu-inner { display: grid; gap: 2px; padding: 6px; border-radius: inherit; max-height: var(--menu-max-height, none); overflow-y: auto; overscroll-behavior: contain; }
.menu-seg { display: flex; min-width: 0; padding: 2px 2px 4px; }
.menu-seg :deep(.segment-track) { flex: 1; max-width: 100%; }
.menu-sep { height: 1px; margin: 2px 10px 4px; background: var(--mat-line); }
.menu-action {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 36px;
  padding: 6px 12px;
  border: 0;
  border-radius: 14px;
  background: transparent;
  color: var(--ink);
  font: inherit;
  font-size: var(--fs-sm);
  text-align: left;
  cursor: pointer;
  transition: background-color .16s ease;
}
.menu-action :deep(svg) { flex: none; color: var(--muted); }
.menu-action:hover, .menu-action:focus-visible { background: color-mix(in srgb, var(--ink) 8%, transparent); outline: none; }
.menu-action:focus-visible { box-shadow: inset 0 0 0 2px var(--focus); }
.menu-action:active { background: color-mix(in srgb, var(--ink) 13%, transparent); }
@media (prefers-reduced-motion: reduce) { .menu-action { transition: none; } }
</style>
