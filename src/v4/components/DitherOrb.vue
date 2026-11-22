<script setup lang="ts">
/* A slowly turning sphere drawn with ordered (Bayer 4×4) dithering: lime dots on black.
   Draws at low resolution and scales up with crisp pixels. Stops when reduced motion is on. */
import { onBeforeUnmount, onMounted, ref } from 'vue';

const props = withDefaults(defineProps<{ size?: number; dot?: number; color?: string }>(), { size: 280, dot: 4, color: '#c6f05c' });
const canvas = ref<HTMLCanvasElement | null>(null);
const BAYER = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5].map((v) => (v + 0.5) / 16);
let raf = 0;

onMounted(() => {
  const el = canvas.value;
  if (!el) return;
  const n = Math.round(props.size / props.dot);
  el.width = n;
  el.height = n;
  const ctx = el.getContext('2d');
  if (!ctx) return;
  const img = ctx.createImageData(n, n);
  const hex = props.color.replace('#', '');
  const [cr, cg, cb] = [0, 2, 4].map((i) => parseInt(hex.slice(i, i + 2), 16));
  const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const draw = (t: number) => {
    const a = t / 4200;
    const lx = Math.cos(a) * 0.75, ly = -0.45, lz = Math.sin(a) * 0.55 + 0.5;
    const ll = Math.hypot(lx, ly, lz);
    const r = n / 2 - 1;
    for (let y = 0; y < n; y++) {
      for (let x = 0; x < n; x++) {
        const nx = (x - n / 2 + 0.5) / r, ny = (y - n / 2 + 0.5) / r;
        const d2 = nx * nx + ny * ny;
        let v = 0;
        if (d2 <= 1) {
          const nz = Math.sqrt(1 - d2);
          const diffuse = Math.max(0, (nx * lx + ny * ly + nz * lz) / ll);
          const rim = Math.pow(1 - nz, 3) * 0.35;
          // faint bands, like a planet surface
          const band = 0.08 * Math.sin((ny * 9 + Math.sin(nx * 3 + a * 2)) * 1.3);
          v = Math.min(1, diffuse * 0.9 + rim + band + 0.04);
        } else if (d2 < 1.25) {
          v = (1.25 - d2) * 0.25; // soft halo
        }
        const on = v > BAYER[(y % 4) * 4 + (x % 4)];
        const i = (y * n + x) * 4;
        img.data[i] = cr; img.data[i + 1] = cg; img.data[i + 2] = cb; img.data[i + 3] = on ? 255 : 0;
      }
    }
    ctx.putImageData(img, 0, 0);
    if (!reduce) raf = requestAnimationFrame(draw);
  };
  draw(0);
});
onBeforeUnmount(() => cancelAnimationFrame(raf));
</script>

<template>
  <canvas ref="canvas" class="orb" :style="{ width: `${size}px`, height: `${size}px` }" aria-hidden="true" />
</template>

<style scoped>
.orb { image-rendering: pixelated; display: block; }
</style>
