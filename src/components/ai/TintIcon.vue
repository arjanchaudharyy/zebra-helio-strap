<script setup lang="ts">
import Icon, { type IconName } from '../Icon.vue';

withDefaults(defineProps<{ name: IconName; tint?: string; size?: number; off?: boolean }>(), { tint: 'var(--accent)', size: 24, off: false });
</script>

<template>
  <span :class="['tint-icon', { off }]" :style="{ '--tint': tint, '--s': `${size}px` }" aria-hidden="true">
    <Icon :name="name" :size="Math.round(size * 0.58)" />
  </span>
</template>

<style scoped>
.tint-icon { display: inline-grid; width: var(--s); height: var(--s); flex: 0 0 var(--s); place-items: center; border-radius: calc(var(--s) * .32);
  background: linear-gradient(180deg, color-mix(in srgb, var(--tint) 30%, transparent), color-mix(in srgb, var(--tint) 14%, transparent));
  box-shadow: inset 0 1px 0 color-mix(in srgb, #fff 18%, transparent), inset 0 0 0 1px color-mix(in srgb, var(--tint) 22%, transparent);
  color: var(--tint); transition: background var(--dur-base) ease, color var(--dur-base) ease; }
.tint-icon.off { background: color-mix(in srgb, var(--ink) 6%, transparent); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--ink) 8%, transparent); color: var(--subtle); }
</style>
