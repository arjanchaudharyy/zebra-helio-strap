<script setup lang="ts">
import GlassSwitch from '../../../components/GlassSwitch.vue';
import { autoPasteOn, autoSupported, setAutoPaste } from '../../../composables/ai/useAutoHandoff';
import { useMessages } from '../../../i18n';
import { aiCardMessages } from './ai.i18n';

const a = useMessages(aiCardMessages);
const supported = autoSupported();
</script>

<template>
  <div v-if="supported" class="s-row">
    <div class="s-row-main">
      <span class="s-row-title">{{ a.autoTitle }}</span>
      <span class="s-row-sub">{{ autoPasteOn ? a.autoOn : a.autoOff }}</span>
    </div>
    <GlassSwitch data-setting="auto-paste" :model-value="autoPasteOn" :aria-label="a.autoTitle" @update:model-value="setAutoPaste" />
  </div>
</template>

<style scoped src="../settings-local.css"></style>
