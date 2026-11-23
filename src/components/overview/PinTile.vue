<script setup lang="ts">
import { computed, inject } from 'vue';
import { RouterLink } from 'vue-router';
import Sparkline from '../Sparkline.vue';
import UsualRange from '../ask/UsualRange.vue';
import { pinHref, pinNumberText } from '../../lib/pinnedMetrics';
import { useMessages } from '../../i18n';
import { pinnedMetricsMessages } from './PinnedMetrics.i18n';
import { PIN_TILES } from './pinTiles';

const props = defineProps<{ id: string }>();
const t = useMessages(pinnedMetricsMessages);
const tiles = inject(PIN_TILES)!;
const tile = computed(() => tiles.value.get(props.id)!);
</script>

<template>
  <RouterLink :to="pinHref(tile.metric)" data-morph-card
    :class="['pin-tile', { 'is-empty': tile.value === '—' }]" :style="{ '--tone': tile.tone }" :aria-label="t.tileAria(tile.label, tile.value)">
    <span class="pin-label"><i class="pin-dot" aria-hidden="true"></i>{{ tile.label }}</span>
    <span class="pin-value"><strong>{{ tile.value }}</strong><small v-if="tile.unit && tile.value !== '—'">{{ tile.unit }}</small></span>
    <span class="pin-when">{{ tile.when ?? tile.coverage }}</span>
    <Sparkline v-if="tile.spark.length > 1" class="pin-spark" :values="tile.spark" :color="tile.tone" :label="`${tile.label} · ${tile.coverage}`" />
    <span v-else class="pin-spark pin-spark-empty" aria-hidden="true"></span>
    <UsualRange class="pin-usual" :metric="tile.id" :format="(v: number) => pinNumberText(tile.metric, v)" />
  </RouterLink>
</template>

<style scoped>
.pin-tile {
  position: relative; display: grid; align-content: start; gap: 4px; min-width: 0; padding: 14px 16px 12px; overflow: hidden;
  border-radius: var(--radius-lg);
  background: radial-gradient(120% 90% at 100% 0%, color-mix(in srgb, var(--tone) 16%, transparent), transparent 60%), var(--mat-card);
  box-shadow: var(--mat-rim), var(--mat-shadow);
  color: inherit; text-decoration: none; transition: translate var(--dur-base) var(--ease-out), box-shadow var(--dur-base) ease;
}
.pin-tile:hover { translate: 0 -2px; }
.pin-tile:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
.pin-label { display: flex; align-items: center; gap: 6px; min-width: 0; overflow: hidden; color: var(--muted); font-size: var(--fs-xs); text-overflow: ellipsis; white-space: nowrap; }
.pin-dot { width: 7px; height: 7px; flex: none; border-radius: 50%; background: var(--tone); box-shadow: 0 0 8px color-mix(in srgb, var(--tone) 60%, transparent); }
.pin-value { display: flex; align-items: baseline; gap: 5px; }
.pin-value strong { color: var(--ink); font-size: var(--fs-3xl); font-weight: 650; font-variant-numeric: tabular-nums; letter-spacing: -.02em; }
.pin-value small { color: var(--muted); font-size: var(--fs-xs); }
.pin-when { color: var(--subtle); font-size: var(--fs-2xs); }
.pin-spark { margin-top: 2px; }
.pin-spark-empty { display: block; height: 52px; border-radius: 10px;
  background: repeating-linear-gradient(135deg, color-mix(in srgb, var(--ink) 4%, transparent) 0 6px, transparent 6px 12px); }
.pin-tile.is-empty { background: var(--mat-card); }
.pin-tile.is-empty .pin-value strong { color: var(--subtle); }
.pin-tile.is-empty .pin-dot { box-shadow: none; opacity: .55; }

.pin-tile::after {
  content: ""; position: absolute; inset: 0; border-radius: inherit; pointer-events: none;
  background: linear-gradient(0deg, color-mix(in srgb, var(--tone) 34%, transparent), color-mix(in srgb, var(--tone) 12%, transparent));
  transform: scaleY(0); transform-origin: 50% 100%; opacity: 0;
}
@media (prefers-reduced-motion: no-preference) {
  .just-pinned .pin-enter-active::after { animation: pin-flood 900ms cubic-bezier(.3, .7, .2, 1) both; }
  .just-pinned .pin-enter-active > * { animation: pin-content 900ms ease both; }
}
@keyframes pin-flood {
  0% { transform: scaleY(0); opacity: 1; }
  50% { transform: scaleY(1); opacity: 1; }
  100% { transform: scaleY(1); opacity: 0; }
}
@keyframes pin-content { 0%, 45% { opacity: 0; translate: 0 6px; } 100% { opacity: 1; translate: 0 0; } }

.pin-usual { margin-top: 4px; }
</style>
