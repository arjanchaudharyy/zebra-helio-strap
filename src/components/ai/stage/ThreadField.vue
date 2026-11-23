<script setup lang="ts">
import { computed } from 'vue';
import { pathD, type ThreadTree } from '../../../lib/aiTask/threads';

const props = defineProps<{ tree: ThreadTree | null; width: number; height: number; lit: boolean; reeled: boolean; order: Record<string, number>; active?: string | null }>();
const trunks = computed(() => props.tree?.trunks.map((q) => pathD([q])) ?? []);
const strands = computed(() => props.tree?.strands.map((s) => ({ id: s.id, d: pathD(s.branch), i: props.order[s.id] ?? 0 })) ?? []);
const right = computed(() => props.tree?.right.map((q) => pathD([q])) ?? []);
const count = computed(() => Math.max(1, strands.value.length));
</script>

<template>
  <svg :class="['threads', { lit, reeled }]" :viewBox="`0 0 ${width} ${height}`" :width="width" :height="height" :style="{ '--n': count }" aria-hidden="true">
    <defs>
      <linearGradient id="branch-fade" gradientUnits="userSpaceOnUse" x1="0" :x2="tree?.origin.x ?? width" y1="0" y2="0">
        <stop offset="0" stop-color="var(--ink)" stop-opacity=".04" />
        <stop offset=".55" stop-color="var(--ink)" stop-opacity=".16" />
        <stop offset="1" stop-color="var(--ink)" stop-opacity=".38" />
      </linearGradient>
      <linearGradient id="thread-fade" x1="0" x2="1" y1="0" y2="0">
        <stop offset="0" stop-color="var(--ink)" stop-opacity=".12" />
        <stop offset="1" stop-color="var(--ink)" stop-opacity=".5" />
      </linearGradient>
    </defs>
    <g class="tree">
      <path v-for="(d, i) in trunks" :key="`t${i}`" :d="d" class="trunk" :data-i="i" pathLength="1" />
      <path v-for="s in strands" :key="s.id" :d="s.d" :class="['branch', { hot: active === s.id }]" :data-id="s.id" :style="{ '--i': s.i }" pathLength="1" />
    </g>
    <g class="right">
      <path v-for="(d, i) in right" :key="`r${i}`" :d="d" />
    </g>
  </svg>
</template>

<style scoped>
.threads { position: absolute; inset: 0; overflow: visible; pointer-events: none; animation: breathe 9s ease-in-out infinite alternate; }
.threads path { fill: none; stroke-linecap: round; transition: opacity 260ms ease, stroke 260ms ease; }
.tree path { stroke-dasharray: 1 1; stroke-dashoffset: 0; }
.trunk { stroke: url(#thread-fade); stroke-width: 1.3; opacity: .85; }
.branch { stroke: url(#branch-fade); stroke-width: .9; }
.right path { stroke: var(--accent); stroke-width: .9; opacity: .4; }
.branch.hot { stroke: var(--accent); opacity: .6; }
.threads.lit .branch { stroke: var(--accent); opacity: .7; transition-delay: calc(var(--i) * 45ms); }
.threads.lit .right path { opacity: .75; }
.threads.reeled .tree path { stroke-dashoffset: 1; }
@keyframes breathe { from { opacity: .75; } to { opacity: 1; } }
@media (prefers-reduced-motion: reduce) { .threads { animation: none; } .threads path { transition: none !important; } }
</style>
