<script setup lang="ts">
import { computed } from 'vue';
import AnatomyFigure from '../anatomy/AnatomyFigure.vue';
import { BODY_PARTS, BODY_PART_PLACES } from '../../lib/logbook/body';
import type { AnatomyRegion, AnatomyRegionState, AnatomyView } from '../../lib/anatomy/types';
import { useBodyPartName, useBodyText } from './body.i18n';

const props = withDefaults(defineProps<{
  modelValue: string[];
  single?: boolean;
  readonly?: boolean;
  marked?: string[];
  showList?: boolean;
}>(), { single: false, readonly: false, marked: () => [], showList: true });
const emit = defineEmits<{ (event: 'update:modelValue', value: string[]): void }>();

const t = useBodyText();
const partName = useBodyPartName();
const views: AnatomyView[] = ['front', 'back'];

function toggle(part: string) {
  if (props.readonly) return;
  const has = props.modelValue.includes(part);
  if (props.single) emit('update:modelValue', has ? [] : [part]);
  else emit('update:modelValue', has ? props.modelValue.filter(p => p !== part) : [...props.modelValue, part]);
}

const regions = computed<AnatomyRegion[]>(() =>
  BODY_PARTS.map((part) => ({ key: part, label: partName.value(part), ...BODY_PART_PLACES[part] })),
);
const states = computed(() => {
  const out: Record<string, AnatomyRegionState> = {};
  for (const part of props.marked) out[part] = { marked: true };
  for (const part of props.modelValue) out[part] = { selected: true };
  return out;
});
</script>

<template>
  <div class="body-map" :class="{ readonly }">
    <div class="figures">
      <figure v-for="view in views" :key="view" :class="['figure', `is-${view}`]">
        <AnatomyFigure
          :view="view"
          :regions="regions"
          :states="states"
          :mode="readonly ? 'none' : 'toggle'"
          float-label
          :aria-label="`${t.figure} · ${view === 'front' ? t.front : t.back}`"
          @activate="toggle"
        />
        <figcaption>{{ view === 'front' ? t.front : t.back }}</figcaption>
      </figure>
    </div>
    <div v-if="showList" class="picked" aria-live="polite">
      <template v-if="modelValue.length">
        <button v-for="part in modelValue" :key="part" type="button" class="chip" :disabled="readonly" @click="toggle(part)">
          {{ partName(part) }}<span v-if="!readonly" aria-hidden="true">×</span>
        </button>
        <button v-if="!readonly && !single && modelValue.length > 1" type="button" class="clear" @click="emit('update:modelValue', [])">{{ t.clear }}</button>
      </template>
      <span v-else class="none">{{ t.none }}</span>
    </div>
  </div>
</template>

<style scoped>
.body-map { display: grid; gap: 10px; min-width: 0; }
.figures { display: flex; justify-content: center; gap: 10px; padding: 12px 10px 10px; border-radius: 18px; background: var(--mat-inset); box-shadow: var(--cap-track-shadow); }
.figure { display: grid; justify-items: center; gap: 6px; margin: 0; flex: 1 1 0; max-width: var(--bm-figure-max, 160px); min-width: 0; }
.figure :deep(svg) { max-height: var(--bm-figure-max-h, none); }
.figure figcaption { padding: 2px 10px; border-radius: 999px; background: color-mix(in srgb, var(--ink) 8%, transparent); color: var(--muted); font-size: var(--fs-xs); font-weight: 600; letter-spacing: .08em; }
.picked { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; min-height: 28px; }
.chip { display: inline-flex; align-items: center; gap: 6px; min-height: 26px; padding: 0 10px; border: 0; border-radius: 999px;
  background: color-mix(in srgb, var(--heart) 16%, transparent); color: var(--ink); font: inherit; font-size: var(--fs-xs); cursor: pointer; }
.chip:disabled { cursor: default; }
.chip span { color: var(--muted); }
.clear { padding: 2px 6px; border: 0; background: transparent; color: var(--muted); font: inherit; font-size: var(--fs-xs); cursor: pointer; }
.clear:hover { color: var(--ink); }
.none { color: var(--subtle); font-size: var(--fs-xs); }
</style>
