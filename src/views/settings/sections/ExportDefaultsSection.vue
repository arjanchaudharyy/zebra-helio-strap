<script setup lang="ts">
import { ref } from 'vue';
import SegmentTrack from '../../../components/SegmentTrack.vue';
import {
  isDefaultExportFormat,
  readDefaultExportFormat,
  writeDefaultExportFormat,
} from '../../../lib/exportScope';
import { useMessages } from '../../../i18n';
import { settingsMessages } from '../../Settings.i18n';
import { deckMessages } from '../deck.i18n';

const t = useMessages(settingsMessages);
const d = useMessages(deckMessages);

const EXPORT_FORMAT_CHOICES = [
  { value: 'json', label: 'JSON' },
  { value: 'csv', label: 'CSV' },
  { value: 'gpx', label: 'GPX' },
];

const defaultExportFormat = ref(readDefaultExportFormat());
const onExportFormatChange = (value: string | number) => {
  const format = String(value);
  if (!isDefaultExportFormat(format)) return;
  defaultExportFormat.value = format;
  writeDefaultExportFormat(format);
};
</script>

<template>
  <section class="s-section" aria-labelledby="export-title">
    <div class="s-section-head"><h3 id="export-title">{{ d.secExport }}</h3></div>
    <div class="s-list">
      <div class="s-row">
        <div class="s-row-main">
          <span class="s-row-title">{{ t.defaultFormatLabel }}</span>
          <span class="s-row-sub">{{ d.exportFormatSub }}</span>
        </div>
        <div class="s-row-control">
          <SegmentTrack
            compact
            :model-value="defaultExportFormat"
            :items="EXPORT_FORMAT_CHOICES"
            :aria-label="t.defaultFormatAria"
            @update:model-value="onExportFormatChange"
          />
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped src="../settings-local.css"></style>
