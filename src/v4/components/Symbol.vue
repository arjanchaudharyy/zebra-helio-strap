<script setup lang="ts">
/* Line icons drawn in the spirit of SF Symbols: 24 px grid, 1.7 stroke, round caps. */
defineProps<{ name: string; size?: number }>();
</script>

<template>
  <svg class="sym" viewBox="0 0 24 24" :width="size ?? 18" :height="size ?? 18" aria-hidden="true">
    <template v-if="name === 'today'"><rect x="3.5" y="4.5" width="17" height="16" rx="3.5" /><path d="M3.5 9.5h17M8 2.8v3.4M16 2.8v3.4" /><circle cx="12" cy="15" r="1.6" class="fill" /></template>
    <template v-else-if="name === 'live'"><path d="M2.5 12h4l2.2-5 4 10 2.6-6.5 1.4 1.5h4.8" /></template>
    <template v-else-if="name === 'sleep'"><path d="M20 14.6A8.3 8.3 0 0 1 9.4 4a8.5 8.5 0 1 0 10.6 10.6Z" /></template>
    <template v-else-if="name === 'heart'"><path d="M12 20.3s-7.8-4.6-7.8-10.4A4.3 4.3 0 0 1 12 7.3a4.3 4.3 0 0 1 7.8 2.6c0 5.8-7.8 10.4-7.8 10.4Z" /></template>
    <template v-else-if="name === 'activity'"><path d="M4 20V13M9.3 20V8.5M14.7 20v-6M20 20V4.5" /></template>
    <template v-else-if="name === 'age'"><path d="M7 3.5h10M7 20.5h10M8 3.5v2.3c0 2.1 4 3.9 4 6.2s-4 4.1-4 6.2v2.3M16 3.5v2.3c0 2.1-4 3.9-4 6.2s4 4.1 4 6.2v2.3" /></template>
    <template v-else-if="name === 'coach'"><path d="M4 6.5A3 3 0 0 1 7 3.5h10a3 3 0 0 1 3 3v7a3 3 0 0 1-3 3h-6.2L6.5 20v-3.5H7a3 3 0 0 1-3-3Z" /><path d="M8.5 10h7" /></template>
    <template v-else-if="name === 'recovery'"><rect x="3" y="7" width="16" height="10" rx="2.8" /><path d="M21 10.5v3" /><path d="M10.5 9 8.5 12.2h3.6L10 15" /></template>
    <template v-else-if="name === 'workouts'"><circle cx="14.5" cy="4.6" r="1.8" /><path d="M9 20.5l2.4-5 2.6 2.2v3.2M7 11.5l3-3.3 3.6.3 2.4 3.2 3 .6M11.4 15.5l1-5.5" /></template>
    <template v-else-if="name === 'insights'"><path d="M12 3.5 13.9 9l5.6.2-4.4 3.5 1.6 5.4L12 15l-4.7 3.1 1.6-5.4L4.5 9.2l5.6-.2Z" /></template>
    <template v-else-if="name === 'settings'"><circle cx="12" cy="12" r="3.2" /><path d="M12 2.8v2.4M12 18.8v2.4M2.8 12h2.4M18.8 12h2.4M5.5 5.5l1.7 1.7M16.8 16.8l1.7 1.7M5.5 18.5l1.7-1.7M16.8 7.2l1.7-1.7" /></template>
    <template v-else-if="name === 'sync'"><path d="M19.5 12a7.5 7.5 0 0 1-13 5.1M4.5 12a7.5 7.5 0 0 1 13-5.1" /><path d="M17.8 3.5v3.6h-3.6M6.2 20.5v-3.6h3.6" /></template>
    <template v-else-if="name === 'sidebar'"><rect x="3" y="4.5" width="18" height="15" rx="3.5" /><path d="M9 4.5v15" /></template>
    <template v-else-if="name === 'theme'"><circle cx="12" cy="12" r="8.5" /><path d="M12 3.5a8.5 8.5 0 0 1 0 17Z" class="fill" /></template>
  </svg>
</template>

<style scoped>
.sym { fill: none; stroke: currentColor; stroke-width: 1.7; stroke-linecap: round; stroke-linejoin: round; flex: none; }
.sym .fill { fill: currentColor; stroke: none; }
</style>
