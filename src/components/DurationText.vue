<script setup lang="ts">
import { computed } from 'vue';
import { formatDurationParts } from '../lib/format';

const props = withDefaults(defineProps<{ minutes: number | null | undefined; empty?: string }>(), { empty: '—' });
const parts = computed(() => formatDurationParts(props.minutes));
</script>

<template>
  <span class="duration-text"><template v-if="parts"><template v-for="(part, i) in parts" :key="i"><b v-if="part.n">{{ part.n }}</b><small v-else>{{ part.u }}</small></template></template><template v-else>{{ empty }}</template></span>
</template>

<style scoped>
.duration-text { display: inline-flex; align-items: baseline; gap: .12em; white-space: nowrap; font-variant-numeric: tabular-nums; }
.duration-text b { font-weight: inherit; }
.duration-text small { margin-right: .18em; color: var(--muted); font-size: .56em; font-weight: 600; }
.duration-text small:last-child { margin-right: 0; }
</style>
