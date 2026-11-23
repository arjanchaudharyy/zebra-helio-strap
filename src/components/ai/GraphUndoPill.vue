<script setup lang="ts">
import { ref, watch } from 'vue';
import Icon from '../Icon.vue';
import { useWidthMorph } from '../../composables/useWidthMorph';

const props = defineProps<{
  canUndo: boolean;
  label: string;
  hint?: string | null;
  seq?: number;
}>();
const emit = defineEmits<{ (event: 'undo'): void }>();

const HINT_MS = 5000;
const pill = ref<HTMLElement | null>(null);
const shown = ref(false);
let timer = 0;
watch(() => props.seq, () => {
  window.clearTimeout(timer);
  shown.value = Boolean(props.hint);
  if (shown.value) timer = window.setTimeout(() => { shown.value = false; }, HINT_MS);
});
watch(() => props.hint, (hint) => { if (!hint) shown.value = false; });
useWidthMorph(pill, () => (shown.value ? props.hint : ''));
</script>

<template>
  <button ref="pill" type="button" :class="['undo-pill', 'glass-control', { 'has-hint': shown && hint }]"
    :disabled="!canUndo" :title="`${label} · Ctrl+Z`" @click="emit('undo')">
    <Icon name="undo" :size="15" />
    <span v-if="shown && hint" :key="seq" class="undo-hint" role="status">{{ hint }}</span>
    <span class="undo-word">{{ label }}</span>
  </button>
</template>

<style scoped>
.undo-pill {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  min-height: 38px;
  padding: 0 16px;
  border-radius: 999px;
  color: var(--ink);
  font-size: var(--fs-sm);
  white-space: nowrap;
  cursor: pointer;
  transition: opacity var(--dur-base) ease, scale var(--dur-fast) var(--ease-out);
}
.undo-pill:disabled { opacity: .4; cursor: default; }
.undo-pill:active:not(:disabled) { scale: .96; }
.undo-hint { max-width: 280px; overflow: hidden; color: var(--muted); text-overflow: ellipsis; animation: undo-hint-in .32s var(--ease-out); }
.undo-hint::after { content: '·'; margin-left: 7px; color: var(--subtle); }
.undo-pill.has-hint .undo-word { color: var(--accent); font-weight: 600; }
@keyframes undo-hint-in { from { opacity: 0; } }
@media (prefers-reduced-motion: reduce) { .undo-hint { animation: none; } }
</style>
