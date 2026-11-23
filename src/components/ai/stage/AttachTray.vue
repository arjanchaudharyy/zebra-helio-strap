<script setup lang="ts">
import TintIcon from '../TintIcon.vue';
import type { AttachItem } from '../../../composables/ai/useAttachFlight';

defineProps<{ items: AttachItem[]; count: number; label: string }>();
</script>

<template>
  <div class="attach-tray" role="status" :aria-label="`${label} ${count}`">
    <div class="tray-panel" aria-hidden="true"><span>{{ label }}</span><b class="count">{{ count }}</b></div>
    <ul class="chips" aria-hidden="true">
      <li v-for="item in items" :key="item.id" class="chip" :data-id="item.id" :style="{ '--tint': item.tint }">
        <TintIcon :name="item.icon" :tint="item.tint" :size="18" />
        <strong>{{ item.title }}</strong>
        <small>{{ item.meta }}</small>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.attach-tray { position: absolute; z-index: 2; padding: 32px 10px 10px; pointer-events: none; }
.tray-panel { position: absolute; inset: 0; border-radius: 16px; background: var(--mat-inset); box-shadow: var(--mat-inset-shadow); }
.tray-panel span, .tray-panel .count { position: absolute; top: 9px; }
.tray-panel span { left: 12px; color: var(--subtle); font-size: var(--fs-2xs); font-weight: 650; }
.tray-panel .count { right: 10px; min-width: 18px; padding: 0 6px; border-radius: 999px; background: color-mix(in srgb, var(--accent) 18%, transparent); color: var(--accent);
  font: 700 10px / 16px var(--font-mono); text-align: center; }
.chips { position: relative; display: grid; grid-template-columns: repeat(auto-fill, minmax(54px, 1fr)); gap: 6px; margin: 0; padding: 0; list-style: none; }
.chip { display: grid; gap: 3px; align-content: start; min-width: 0; height: 60px; padding: 7px 7px 6px; border-radius: 11px;
  background: radial-gradient(130% 70% at 50% 0%, color-mix(in srgb, var(--tint) 16%, transparent), transparent 70%), var(--mat-raised);
  box-shadow: var(--mat-raised-rim), inset 0 0 0 1px color-mix(in srgb, var(--tint) 14%, transparent), 0 10px 18px -12px rgba(0, 0, 0, .7); }
.chip strong, .chip small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.chip strong { margin-top: 1px; font-size: 11px; font-weight: 700; line-height: 1.2; letter-spacing: -.01em; }
.chip small { color: var(--subtle); font-size: 10px; line-height: 1.2; }
</style>
