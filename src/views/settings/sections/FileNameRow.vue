<script setup lang="ts">
import { computed, ref } from 'vue';
import SegmentTrack from '../../../components/SegmentTrack.vue';
import { useMessages } from '../../../i18n';
import { exportFileStems, readFileNameRule, writeFileNameRule, isFileNameRule, type FileNameRule } from '../../../lib/aiTask/fileName';
import { fileNameMessages } from './fileName.i18n';

const n = useMessages(fileNameMessages);

const fileNameRule = ref<FileNameRule>(readFileNameRule());
const ruleItems = computed(() => [
  { value: 'range_content', label: n.value.ruleRange },
  { value: 'task_time', label: n.value.ruleTask },
  { value: 'app_date', label: n.value.ruleApp },
]);
const onRuleChange = (value: string | number) => {
  if (!isFileNameRule(value)) return;
  fileNameRule.value = value;
  writeFileNameRule(value);
};
const namePreview = computed(() => {
  const now = new Date();
  const iso = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const start = new Date(now.getTime() - 13 * 86400000);
  const stems = exportFileStems(fileNameRule.value, {
    start: iso(start), end: iso(now), categories: ['sleep', 'heart_rate', 'recovery', 'training', 'body', 'workout'],
    title: n.value.exampleTitle, now,
  });
  return `${stems.data}.json · ${stems.prompt}.txt`;
});
</script>

<template>
  <div class="s-row s-row-stack">
    <div class="s-row-main">
      <span class="s-row-title">{{ n.nameTitle }}</span>
      <span class="s-row-sub">{{ n.nameSub }}</span>
    </div>
    <div class="s-row-control">
      <SegmentTrack compact :model-value="fileNameRule" :items="ruleItems" :aria-label="n.nameTitle" @update:model-value="onRuleChange" />
    </div>
    <p class="name-preview"><span>{{ n.example }}</span><code>{{ namePreview }}</code></p>
  </div>
</template>

<style scoped src="../settings-local.css"></style>
<style scoped>
.s-row-stack { flex-wrap: wrap; }
.name-preview { display: flex; flex: 1 1 100%; min-width: 0; gap: 8px; margin: 2px 0 0; color: var(--subtle); font-size: var(--fs-xs); }
.name-preview code { min-width: 0; overflow: hidden; color: var(--muted); font-family: var(--font-mono); text-overflow: ellipsis; white-space: nowrap; }
</style>
