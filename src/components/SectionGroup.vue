<script setup lang="ts">
import GlyphTile from './GlyphTile.vue';
import type { DesignIconName } from './DesignIcon.vue';
import type { GlyphTone } from '../lib/glyphs';

defineProps<{
  title: string;
  summary?: string;
  icon: DesignIconName;
  tone?: GlyphTone;
}>();
</script>

<template>
  <section class="section-group" :aria-label="title">
    <header class="section-group-head">
      <GlyphTile :name="icon" :tone="tone" :size="36" />
      <span class="section-group-copy">
        <h2>{{ title }}</h2>
        <small v-if="summary">{{ summary }}</small>
      </span>
    </header>
    <slot />
  </section>
</template>

<style scoped>
.section-group { display: grid; gap: 12px; min-width: 0; margin-top: 6px; }
.section-group-head { display: flex; min-width: 0; align-items: center; gap: 12px; padding: 0 4px; }
.section-group-copy { display: grid; min-width: 0; gap: 1px; }
.section-group-copy h2 { margin: 0; color: var(--ink); font-size: var(--fs-lg); font-weight: 700; line-height: 1.3; }
.section-group-copy small { overflow: hidden; color: var(--subtle); font-size: var(--fs-xs); text-overflow: ellipsis; white-space: nowrap; }
</style>
