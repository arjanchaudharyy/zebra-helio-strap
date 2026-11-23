<script setup lang="ts">
import DeviceMarquee from '../../components/DeviceMarquee.vue';

</script>

<template>
  <div class="film-wall devices" aria-hidden="true">
    <DeviceMarquee />
  </div>
</template>

<style scoped>
.film-wall { position: fixed; inset: 0; z-index: 9000; display: grid; align-content: center; overflow: hidden; background: var(--bg); }
.film-wall :deep(.device-marquee) { gap: 44px; mask-image: none; }
.film-wall :deep(.marquee-pass) { gap: 54px; padding-right: 54px; }
.film-wall :deep(.marquee-track) { animation-duration: 26s; }
.film-wall :deep(.dir-rtl .marquee-track) { animation-duration: 30s; }
.film-wall :deep(img) { width: 210px; height: 210px; object-fit: contain; filter: drop-shadow(0 18px 24px rgba(0, 0, 0, .45)); }
</style>
