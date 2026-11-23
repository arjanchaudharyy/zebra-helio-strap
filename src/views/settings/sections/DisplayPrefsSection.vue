<script setup lang="ts">
import { computed, ref } from 'vue';
import SegmentTrack from '../../../components/SegmentTrack.vue';
import { useTheme, type ThemeMode } from '../../../composables/useTheme';
import {
  DISTANCE_UNITS,
  distanceUnit,
  distanceUnitOptionLabel,
  setDistanceUnit,
  type DistanceUnit,
} from '../../../lib/units';
import { useMessages } from '../../../i18n';
import { settingsMessages } from '../../Settings.i18n';
import { deckMessages } from '../deck.i18n';
import DateTimeRows from './DateTimeRows.vue';

const t = useMessages(settingsMessages);
const d = useMessages(deckMessages);
const { themeMode, resolvedTheme, pickTheme } = useTheme();

const distanceUnitOptions = computed(() =>
  DISTANCE_UNITS.map((value) => ({ value, label: distanceUnitOptionLabel(value) })));
const themeOptions = computed(() => [
  { value: 'dark' as ThemeMode, label: d.value.themeDark, icon: 'moon' as const },
  { value: 'light' as ThemeMode, label: d.value.themeLight, icon: 'sun' as const },
  { value: 'system' as ThemeMode, label: d.value.themeSystem, icon: 'monitor' as const },
]);
const themeTrack = ref<{ $el: HTMLElement } | null>(null);
const chooseDistanceUnit = (value: string | number) => setDistanceUnit(String(value) as DistanceUnit);
const chooseTheme = (value: string | number) => {
  const index = themeOptions.value.findIndex((option) => option.value === value);
  const rect = themeTrack.value?.$el.querySelectorAll<HTMLElement>('.segment-item')[index]?.getBoundingClientRect();
  pickTheme(String(value) as ThemeMode, rect ? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 } : undefined);
};
</script>

<template>
  <section class="s-section">
    <div class="s-list">
      <div class="s-row">
        <div class="s-row-main">
          <span class="s-row-title">{{ d.themeLabel }}</span>
          <span class="s-row-sub">{{ resolvedTheme === 'dark' ? d.themeDark : d.themeLight }}<template v-if="themeMode === 'system'"> · {{ d.themeSystem }}</template></span>
        </div>
        <div class="s-row-control">
          <SegmentTrack ref="themeTrack" compact icon-only :items="themeOptions" :model-value="themeMode" :aria-label="d.themeLabel" @update:model-value="chooseTheme" />
        </div>
      </div>
      <div class="s-row">
        <div class="s-row-main"><span class="s-row-title">{{ t.distanceUnitLabel }}</span></div>
        <div class="s-row-control">
          <SegmentTrack compact :items="distanceUnitOptions" :model-value="distanceUnit" :aria-label="t.distanceUnitLabel" @update:model-value="chooseDistanceUnit" />
        </div>
      </div>
      <DateTimeRows />
    </div>
  </section>
</template>

<style scoped src="../settings-local.css"></style>
