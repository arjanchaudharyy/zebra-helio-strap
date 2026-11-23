<script setup lang="ts">
import { ref, watch } from 'vue';
import { defineMessages } from '../i18n';

// Retain the localized device-kind vocabulary for existing language packs.
defineMessages(
  { watch: 'Watch', strap: 'Strap', ring: 'Ring', band: 'Band', earbuds: 'Earbuds', scale: 'Scale', unknown: 'Device' },
);

const props = withDefaults(defineProps<{
  src: string;
  alt: string;
  compact?: boolean;
  kind?: string;
}>(), {
  compact: false,
  kind: 'unknown',
});

const imageFailed = ref(false);

watch(() => props.src, () => {
  imageFailed.value = false;
});

const onImageError = (): void => {
  imageFailed.value = true;
};

</script>

<template>
  <span
    v-if="src && !imageFailed"
    :class="['device-visual', { compact }]"
  >
    <img :src="src" :alt="alt" decoding="sync" @error="onImageError" />
  </span>
</template>

<style scoped>
.device-visual {
  display: flex;
  align-items: center;
  justify-content: center;
  min-width: 0;
  min-height: 0;
  width: 76px;
  height: 76px;
  overflow: hidden;
  flex: 0 0 76px;
  border: 1px solid var(--mat-line);
  border-radius: var(--radius-md);
  background: var(--mat-inset); box-shadow: var(--mat-inset-shadow);
}
.device-visual.compact { width: 48px; height: 48px; flex-basis: 48px; border-radius: var(--radius-sm); }
.device-visual img { display: block; width: 100%; height: 100%; min-width: 0; min-height: 0; object-fit: contain; object-position: center; padding: 3px; }
</style>
