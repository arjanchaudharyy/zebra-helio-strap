<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';
import L from 'leaflet';
import 'leaflet/dist/leaflet.css';
import { openUrl } from '@tauri-apps/plugin-opener';
import { buildRouteRuns, type RouteCanvas, type TrackPoint } from '../../lib/workoutRoute';
import { MAP_TILES } from '../../lib/mapTiles';
import { resolvedTheme } from '../../composables/useTheme';
import { deferResize } from '../../lib/motion/budget';

const props = defineProps<{ canvas: RouteCanvas; label: string; cursor?: TrackPoint | null }>();
const emit = defineEmits<{ (event: 'failed'): void }>();

const FIRST_TILE_TIMEOUT_MS = 10_000;
const TILE_RETRY_MS = 1_500;

const host = ref<HTMLElement | null>(null);
let map: L.Map | null = null;
let routeLayer: L.LayerGroup | null = null;
let cursorMarker: L.Marker | null = null;
let resizeObserver: ResizeObserver | null = null;
let tileLoads = 0;
let tileErrors = 0;
let firstTileTimer = 0;
let sizeFrame = 0;
let framed = false;

const resolveColor = (value: string): string => {
  const match = /^var\((--[\w-]+)\)$/.exec(value.trim());
  if (!match || !host.value) return value;
  return getComputedStyle(host.value).getPropertyValue(match[1]).trim() || value;
};

const drawRoute = () => {
  if (!map) return;
  routeLayer?.remove();
  const layer = L.layerGroup();
  const runs = buildRouteRuns(props.canvas.segments);
  const casing = resolveColor('var(--mat-card-solid)');
  if (runs.length) {
    L.polyline(runs.map((run) => run.points), { color: casing, weight: 8, opacity: .85, interactive: false, lineCap: 'round', lineJoin: 'round' }).addTo(layer);
  }
  for (const run of runs) {
    L.polyline(run.points, { color: resolveColor(run.color), weight: 4.5, opacity: 1, interactive: false, lineCap: 'round', lineJoin: 'round' }).addTo(layer);
  }
  for (const marker of props.canvas.pauseMarkers) {
    L.circleMarker([marker.latitude, marker.longitude], { radius: 5, color: resolveColor('var(--route-amber)'), weight: 2, fillColor: casing, fillOpacity: 1, interactive: false }).addTo(layer);
  }
  const { start, end } = props.canvas;
  L.circleMarker([start.latitude, start.longitude], { radius: 7, color: casing, weight: 2, fillColor: resolveColor('var(--route-mint)'), fillOpacity: 1, interactive: false }).addTo(layer);
  L.circleMarker([end.latitude, end.longitude], { radius: 7, color: casing, weight: 2, fillColor: resolveColor('var(--route-coral)'), fillOpacity: 1, interactive: false }).addTo(layer);
  layer.addTo(map);
  routeLayer = layer;
};

const CURSOR_ICON = L.divIcon({ className: 'zb-route-cursor', html: '<i></i>', iconSize: [34, 34], iconAnchor: [17, 17] });

const placeCursor = (point: TrackPoint | null | undefined) => {
  if (!map) return;
  if (!point) {
    cursorMarker?.remove();
    cursorMarker = null;
    return;
  }
  const at: L.LatLngTuple = [point.latitude, point.longitude];
  if (cursorMarker) cursorMarker.setLatLng(at);
  else cursorMarker = L.marker(at, { icon: CURSOR_ICON, interactive: false, keyboard: false, zIndexOffset: 1000 }).addTo(map);
};

const routeBounds = (): L.LatLngBounds => {
  const points: L.LatLngTuple[] = [[props.canvas.start.latitude, props.canvas.start.longitude], [props.canvas.end.latitude, props.canvas.end.longitude]];
  for (const segment of props.canvas.segments) points.push([segment.to.latitude, segment.to.longitude]);
  return L.latLngBounds(points);
};

const giveUpIfNothingLoads = () => {
  if (tileLoads === 0 && tileErrors >= 4) emit('failed');
};

const retryTile = (event: L.TileErrorEvent) => {
  const img = event.tile as HTMLImageElement & { zbRetried?: boolean };
  if (img.zbRetried) {
    tileErrors += 1;
    giveUpIfNothingLoads();
    return;
  }
  img.zbRetried = true;
  const src = img.src;
  window.setTimeout(() => {
    if (!map) return;
    img.src = `${src}${src.includes('?') ? '&' : '?'}retry=1`;
  }, TILE_RETRY_MS);
};

const frameRoute = () => {
  if (!map || !host.value?.clientWidth || !host.value.clientHeight) return false;
  map.fitBounds(routeBounds(), { padding: [28, 28], maxZoom: 17 });
  framed = true;
  return true;
};

const fitSize = () => {
  if (!map) return;
  map.invalidateSize({ animate: false });
  if (!framed) frameRoute();
};
const onResize = () => {
  if (sizeFrame) return;
  sizeFrame = requestAnimationFrame(() => {
    sizeFrame = 0;
    if (!deferResize(fitSize)) fitSize();
  });
};

const enableWheel = () => map?.scrollWheelZoom.enable();
const disableWheel = () => map?.scrollWheelZoom.disable();

const openAttribution = () => {
  openUrl(MAP_TILES.attributionUrl).catch(() => {
  });
};

onMounted(() => {
  if (!host.value) return;
  map = L.map(host.value, {
    preferCanvas: true,
    attributionControl: false,
    scrollWheelZoom: false,
    zoomSnap: .5,
    maxZoom: MAP_TILES.maxZoom,
  });
  const tiles = L.tileLayer(MAP_TILES.url, {
    maxZoom: MAP_TILES.maxZoom,
    keepBuffer: 1,
    updateWhenZooming: false,
  });
  tiles.on('tileload', () => { tileLoads += 1; window.clearTimeout(firstTileTimer); });
  tiles.on('tileerror', retryTile);
  tiles.addTo(map);
  if (!frameRoute()) map.setView([props.canvas.start.latitude, props.canvas.start.longitude], 14);
  firstTileTimer = window.setTimeout(() => { if (tileLoads === 0) emit('failed'); }, FIRST_TILE_TIMEOUT_MS);
  map.on('click', enableWheel);
  host.value.addEventListener('focusin', enableWheel);
  host.value.addEventListener('mouseleave', disableWheel);
  drawRoute();
  resizeObserver = new ResizeObserver(onResize);
  resizeObserver.observe(host.value);
});

watch(resolvedTheme, () => requestAnimationFrame(drawRoute));
watch(() => props.cursor, placeCursor);
watch(() => props.canvas, () => {
  drawRoute();
  framed = false;
  frameRoute();
});

onBeforeUnmount(() => {
  window.clearTimeout(firstTileTimer);
  cancelAnimationFrame(sizeFrame);
  resizeObserver?.disconnect();
  host.value?.removeEventListener('focusin', enableWheel);
  host.value?.removeEventListener('mouseleave', disableWheel);
  map?.remove();
  map = null;
  routeLayer = null;
  cursorMarker = null;
});
</script>

<template>
  <div class="zb-route-map">
    <div ref="host" class="zb-route-map-host" role="region" :aria-label="label"></div>
    <button type="button" class="zb-route-map-credit" @click="openAttribution">{{ MAP_TILES.attribution }}</button>
  </div>
</template>

<style>
.zb-route-map { position: absolute; inset: 0; isolation: isolate; }
.zb-route-map-host { position: absolute; inset: 0; background: var(--mat-inset); font: inherit; }
.zb-route-map-host:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
:root[data-theme='dark'] .zb-route-map .leaflet-tile-pane { filter: invert(1) hue-rotate(180deg) brightness(.92) contrast(.88) saturate(.35); }
.zb-route-map .leaflet-control-zoom { border: 1px solid var(--mat-glass-line); border-radius: 10px; overflow: hidden; box-shadow: none; }
.zb-route-map .leaflet-control-zoom a { background: var(--mat-glass-strong); color: var(--ink); border-bottom-color: var(--mat-glass-line); }
.zb-route-map .leaflet-control-zoom a:hover { background: var(--mat-card-solid); }
.zb-route-map-credit { position: absolute; top: 10px; right: 10px; z-index: 1000; padding: 3px 8px; border: 1px solid var(--mat-glass-line); border-radius: 8px; background: var(--mat-glass-strong); color: var(--muted); font: inherit; font-size: var(--fs-2xs); cursor: pointer; }
.zb-route-map-credit:hover { color: var(--ink); }
.zb-route-map .zb-route-cursor { border-radius: 50%; background: color-mix(in srgb, var(--ink) 16%, transparent); pointer-events: none; }
.zb-route-map .zb-route-cursor i { position: absolute; inset: 10px; border: 3px solid var(--mat-card-solid); border-radius: 50%; background: var(--ink); }
</style>
