<script setup lang="ts">
export interface MuscleChip {
  key: string;
  label: string;
  sets: number;
  color: string;
}

defineProps<{
  primary: MuscleChip[];
  secondary: MuscleChip[];
  lit: ReadonlySet<string>;
  focused: boolean;
  setsCount: (sets: number) => string;
  secondaryLabel: string;
  label: string;
}>();
const emit = defineEmits<{
  (event: 'hover', key: string | null): void;
  (event: 'pin', key: string): void;
}>();
</script>

<template>
  <div class="chips" :aria-label="label">
    <ul class="chip-row">
      <li v-for="item in primary" :key="item.key">
        <button
          type="button"
          class="chip"
          :class="{ lit: lit.has(item.key), dim: focused && !lit.has(item.key) }"
          :aria-pressed="lit.has(item.key)"
          @pointerenter="emit('hover', item.key)"
          @pointerleave="emit('hover', null)"
          @focus="emit('hover', item.key)"
          @blur="emit('hover', null)"
          @click="emit('pin', item.key)"
        >
          <span class="dot" :style="{ background: item.color }" aria-hidden="true" />
          <span class="c-name">{{ item.label }}</span>
          <span class="c-sets">{{ setsCount(item.sets) }}</span>
        </button>
      </li>
    </ul>
    <ul v-if="secondary.length" class="chip-row secondary" :aria-label="secondaryLabel">
      <li class="sec-label">{{ secondaryLabel }}</li>
      <li v-for="item in secondary" :key="item.key">
        <button
          type="button"
          class="chip hollow"
          :class="{ lit: lit.has(item.key), dim: focused && !lit.has(item.key) }"
          :aria-pressed="lit.has(item.key)"
          @pointerenter="emit('hover', item.key)"
          @pointerleave="emit('hover', null)"
          @focus="emit('hover', item.key)"
          @blur="emit('hover', null)"
          @click="emit('pin', item.key)"
        >
          <span class="dot" :style="{ borderColor: item.color }" aria-hidden="true" />
          <span class="c-name">{{ item.label }}</span>
          <span class="c-sets">{{ setsCount(item.sets) }}</span>
        </button>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.chips { display: grid; gap: 8px; min-width: 0; }
.chip-row { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin: 0; padding: 0; list-style: none; }
.sec-label { margin-right: 2px; color: var(--subtle); font-size: var(--fs-2xs); font-weight: 600; }
.chip {
  display: inline-flex; align-items: center; gap: 6px; padding: 4px 10px 4px 8px;
  border: 1px solid var(--mat-line); border-radius: 999px; background: var(--mat-inset);
  color: var(--muted); font: inherit; font-size: var(--fs-xs); cursor: pointer;
  transition: opacity .18s ease, background-color .18s ease, border-color .18s ease;
}
.chip:hover, .chip.lit { border-color: color-mix(in srgb, var(--training) 55%, var(--mat-line)); background: color-mix(in srgb, var(--training) 12%, var(--mat-inset)); }
.chip.dim { opacity: .45; }
.chip:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
.dot { width: 8px; height: 8px; border-radius: 50%; flex: 0 0 auto; }
.hollow .dot { box-sizing: border-box; border: 2px solid; background: transparent; }
.c-name { color: var(--ink); font-weight: 500; }
.c-sets { font-family: var(--font-mono); font-variant-numeric: tabular-nums; color: var(--subtle); }
@media (prefers-reduced-motion: reduce) { .chip { transition: none; } }
</style>
