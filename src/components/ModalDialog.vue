<script setup lang="ts">
import { onBeforeUnmount, onMounted, onUnmounted, ref, watch } from 'vue';
import { dialogFlight, originOf, type FlightOrigin } from '../lib/motion/dialogFlight';

const props = defineProps<{
  labelledby: string;
  maxWidth?: number;
  origin?: HTMLElement | null;
  bare?: boolean;
}>();
const emit = defineEmits<{ (event: 'close'): void }>();
const panel = ref<HTMLElement | null>(null);
const backdrop = ref<HTMLElement | null>(null);
let returnFocus: HTMLElement | null = null;
let origin: FlightOrigin | null = null;
let leaving: { backdrop: HTMLElement; panel: HTMLElement } | null = null;

const isTopmost = () => {
  const dialogs = document.querySelectorAll('[data-modal-dialog]');
  return dialogs[dialogs.length - 1] === panel.value;
};
const focusableElements = () => Array.from(panel.value?.querySelectorAll<HTMLElement>(
  'button, a[href], input, select, textarea, [tabindex]',
) ?? []).filter((element) => element.tabIndex >= 0
  && !element.matches(':disabled') && !element.closest('[inert]') && element.getClientRects().length > 0);

const onKeydown = (event: KeyboardEvent) => {
  if (!isTopmost()) return;
  if (event.key === 'Escape') {
    event.preventDefault();
    event.stopPropagation();
    emit('close');
  } else if (event.key === 'Tab') {
    const elements = focusableElements();
    const first = elements[0];
    const last = elements[elements.length - 1];
    if (!first) {
      event.preventDefault();
      panel.value?.focus();
    } else if (event.shiftKey && (document.activeElement === first || document.activeElement === panel.value)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }
};
const keepFocusInside = (event: FocusEvent) => {
  if (isTopmost() && !panel.value?.contains(event.target as Node)) panel.value?.focus();
};

watch(() => props.origin, (next) => {
  if (next) origin = originOf(next);
});

onMounted(() => {
  returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  origin = originOf(props.origin ?? returnFocus);
  if (backdrop.value && panel.value) void dialogFlight.open(backdrop.value, panel.value, origin, () => emit('close'));
  // Start at the heading/content instead of scrolling a long dialog to its first action.
  panel.value?.focus({ preventScroll: true });
  document.addEventListener('keydown', onKeydown, true);
  document.addEventListener('focusin', keepFocusInside);
});
onBeforeUnmount(() => {
  leaving = backdrop.value && panel.value ? { backdrop: backdrop.value, panel: panel.value } : null;
  document.removeEventListener('keydown', onKeydown, true);
  document.removeEventListener('focusin', keepFocusInside);
  if (returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
});
onUnmounted(() => {
  if (leaving) dialogFlight.close(leaving.backdrop, leaving.panel, origin);
  leaving = null;
});
</script>

<template>
  <Teleport to="body">
    <div ref="backdrop" class="dialog-backdrop" @click.self="emit('close')">
      <section ref="panel" :class="['dialog-panel', { 'is-bare': bare }]" :style="maxWidth ? { maxWidth: `${maxWidth}px` } : undefined" data-modal-dialog role="dialog" aria-modal="true"
        :aria-labelledby="labelledby" tabindex="-1">
        <span class="dialog-glass" aria-hidden="true"></span>
        <div class="dialog-scroll"><slot /></div>
      </section>
    </div>
  </Teleport>
</template>

<style scoped>
.dialog-backdrop { position: fixed; inset: 0; z-index: 2100; display: grid; place-items: center; padding: 20px; }
.dialog-backdrop::before { content: ''; position: absolute; inset: 0; z-index: 0; pointer-events: none; background: rgba(0, 0, 0, .5); -webkit-backdrop-filter: blur(14px) saturate(1.1); backdrop-filter: blur(14px) saturate(1.1); }
.dialog-backdrop.is-ghost-panel-only::before { opacity: 0 !important; backdrop-filter: none; -webkit-backdrop-filter: none; }
.dialog-panel { position: relative; z-index: 1; isolation: isolate; display: flex; width: 100%; max-width: 560px; min-width: 0; max-height: calc(100vh / var(--ui-scale, 1) - 40px); border-radius: var(--radius-lg); color: var(--ink); overflow-wrap: anywhere; }
.dialog-panel:focus, .dialog-panel:focus-visible { outline: none; }
.dialog-glass { position: absolute; inset: 0; z-index: 0; border: 1px solid var(--mat-glass-line); border-radius: inherit; background: var(--mat-glass-strong); -webkit-backdrop-filter: var(--mat-glass-blur); backdrop-filter: var(--mat-glass-blur); box-shadow: var(--mat-glass-shadow); pointer-events: none; }
.dialog-scroll { position: relative; z-index: 1; flex: 1 1 auto; min-width: 0; overflow-y: auto; overflow-x: clip; overscroll-behavior: contain; padding: 20px; border-radius: inherit; }
.is-bare > .dialog-glass { display: none; }
.is-bare > .dialog-scroll { padding: 0; overflow: visible; }
.dialog-panel::after { content: ''; position: absolute; inset: 0; border-radius: inherit; background: var(--mat-card-solid); z-index: -1; pointer-events: none; opacity: 0; transition: opacity 160ms ease-out; }
.dialog-panel.is-flying::after { opacity: 1; transition: none; }
.is-flying > .dialog-glass { backdrop-filter: none; -webkit-backdrop-filter: none; }
.is-flying > .dialog-glass, .dialog-panel.is-flying::after { right: auto; bottom: auto; width: var(--flight-w, 100%); height: var(--flight-h, 100%); }
.is-bare::after { display: none; }
</style>

<style>
[data-dialog-source]:not([data-dialog-source='fading']) { opacity: 0 !important; transition: none !important; }
[data-dialog-source='fading'] { transition: none !important; }
</style>
