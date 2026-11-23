<script setup lang="ts">
import GlassSwitch from '../../../components/GlassSwitch.vue';
import { onMounted, ref } from 'vue';
import { backend, isDesktop, toUserMessage } from '../../../lib/bridge';
import type { AiTaskSummary } from '../../../lib/bridge/types';
import { useAiTaskDraft } from '../../../composables/useAiTaskDraft';
import { useAiTaskLibrary } from '../../../composables/useAiTaskLibrary';
import { defineMessages, useMessages } from '../../../i18n';
import { aiCardMessages } from './ai.i18n';

const t = useMessages(defineMessages(
  {
    title: 'Open tasks to MCP',
    empty: 'No saved tasks yet. Send something to an AI once and the task appears here.',
  },
));
const a = useMessages(aiCardMessages);
const { taskList, loadTaskList } = useAiTaskLibrary();
const { draft, setMcpShared } = useAiTaskDraft();
const busy = ref<string | null>(null);
const error = ref<string | null>(null);
onMounted(() => { if (isDesktop()) void loadTaskList(); });
const toggle = async (task: AiTaskSummary) => {
  busy.value = task.id;
  error.value = null;
  try {
    const full = await backend.aiTaskGet(task.id);
    await backend.aiTaskSave({ ...full, mcp_shared: !task.mcp_shared });
    if (draft.value.id === task.id) setMcpShared(!task.mcp_shared);
    await loadTaskList();
  } catch (e) {
    error.value = toUserMessage(e, '');
  } finally {
    busy.value = null;
  }
};
</script>

<template>
  <div class="s-row is-block">
    <div class="s-row-main">
      <span class="s-row-title">{{ t.title }}</span>
      <span class="s-row-sub scope-hint">{{ a.scopesHint }}</span>
    </div>
    <p v-if="!taskList.length" class="s-row-sub empty">{{ t.empty }}</p>
    <ul v-else class="scopes">
      <li v-for="task in taskList" :key="task.id">
        <span class="name">{{ task.title }}</span>
        <GlassSwitch :model-value="task.mcp_shared" :aria-label="task.title"
          :disabled="busy === task.id" @update:model-value="toggle(task)" />
      </li>
    </ul>
    <p v-if="error" class="hint-line" role="alert">{{ error }}</p>
  </div>
</template>

<style scoped src="../settings-local.css"></style>
<style scoped>
.scopes { display: grid; margin: 6px 0 0; padding: 0; list-style: none; }
.scopes li { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 7px 0; }
.scopes li + li { border-top: 1px solid var(--mat-line); }
.name { min-width: 0; overflow: hidden; color: var(--ink); font-size: var(--fs-sm); text-overflow: ellipsis; white-space: nowrap; }
.empty { margin-top: 8px; }
.scope-hint { font-variant-ligatures: none; font-feature-settings: "liga" 0, "calt" 0; }
</style>
