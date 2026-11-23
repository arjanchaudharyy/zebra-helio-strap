<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { useRouter } from 'vue-router';
import Icon from '../Icon.vue';
import { onMotionEscape } from '../../lib/motion/interrupt';
import { openSheet } from '../../lib/motion/sheet';
import { useBridgeText } from './bridge/bridge.i18n';

const props = defineProps<{ name: string }>();
const router = useRouter();
const t = useBridgeText();
const root = ref<HTMLElement | null>(null);
const close = () => { void router.push('/ai'); };

let releaseEscape: (() => void) | null = null;
onMounted(() => {
  releaseEscape = onMotionEscape(() => { close(); return true; });
  if (root.value) openSheet(root.value, props.name);
});
onBeforeUnmount(() => releaseEscape?.());

const label = computed(() => t.value.close);
</script>

<template>
  <div ref="root" class="ai-sheet" role="dialog" aria-modal="true" :data-sheet-name="name">
    <div class="sheet-veil" aria-hidden="true" @click="close"></div>
    <section class="sheet-panel">
      <button type="button" class="sheet-close" :aria-label="label" @click="close"><Icon name="x" :size="16" /></button>
      <div class="sheet-body"><slot /></div>
    </section>
  </div>
</template>

<style scoped>
.ai-sheet { position: fixed; inset: 0; z-index: 25; display: grid; justify-items: center; align-items: start; padding: 84px 24px 24px; pointer-events: none; }
.sheet-veil { position: absolute; inset: 0; pointer-events: auto; background: color-mix(in srgb, var(--canvas) 46%, transparent);
  -webkit-backdrop-filter: blur(18px) saturate(1.1); backdrop-filter: blur(18px) saturate(1.1); }
.sheet-panel { position: relative; display: flex; width: min(960px, 100%); max-height: 100%; min-height: 0; pointer-events: auto; border: 1px solid var(--mat-glass-line); border-radius: 28px;
  background: var(--mat-glass-strong); box-shadow: var(--mat-glass-shadow), 0 40px 90px -40px rgba(0, 0, 0, .7); transform-origin: 50% 50%; }
.sheet-body { flex: 1 1 auto; min-width: 0; margin: 18px 6px 18px 0; overflow-y: auto; overscroll-behavior: contain; border-radius: 0;
  scrollbar-width: none; }
.sheet-body :deep(.page) { padding-top: 4px; padding-bottom: 10px; }
.sheet-close { position: absolute; top: 14px; right: 14px; z-index: 2; display: grid; place-items: center; width: 34px; height: 34px; padding: 0; border: 1px solid var(--mat-glass-line);
  border-radius: 50%; background: var(--mat-glass-strong); color: var(--muted); cursor: pointer; transition: color 140ms ease, scale 160ms ease; }
.sheet-close:hover { color: var(--ink); scale: 1.06; }
.sheet-close:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
@media (max-width: 700px) { .ai-sheet { padding: 76px 10px 10px; } .sheet-panel { border-radius: 22px; } }
</style>
<style>
[data-sheet-source] { opacity: 0 !important; transition: none !important; }
</style>
