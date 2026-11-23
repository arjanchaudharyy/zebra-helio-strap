<script setup lang="ts">
import { ref } from 'vue';
import { backend, isDesktop, toUserMessage } from '../../../lib/bridge';
import { dayKey } from '../../../lib/aiTask/bridgeScale';
import { today } from '../../../lib/currentDay';
import { useMessages } from '../../../i18n';
import { archiveCardMessages } from './archive.i18n';

const a = useMessages(archiveCardMessages);
const busy = ref(false);
const note = ref('');
const error = ref('');

const takeAll = async () => {
  if (busy.value) return;
  error.value = ''; note.value = '';
  try {
    const { save } = await import('@tauri-apps/plugin-dialog');
    const path = await save({
      defaultPath: `ZeppBridge-my-records-${dayKey(today())}.json`,
      filters: [{ name: 'JSON', extensions: ['json'] }],
    });
    if (!path) return;
    busy.value = true;
    note.value = a.value.takeDone(await backend.exportMyRecords(path));
  } catch (cause) {
    error.value = toUserMessage(cause, a.value.takeFailed);
  } finally {
    busy.value = false;
  }
};
</script>

<template>
  <div class="s-row">
    <div class="s-row-main">
      <span class="s-row-title">{{ a.takeTitle }}</span>
      <span class="s-row-sub">{{ a.takeSub }}</span>
      <span v-if="note" class="s-row-sub" role="status">{{ note }}</span>
      <span v-if="error" class="s-row-sub take-error" role="alert">{{ error }}</span>
    </div>
    <div class="s-row-control">
      <button class="button secondary" type="button" :disabled="busy || !isDesktop()" @click="takeAll">{{ a.takeButton }}</button>
    </div>
  </div>
</template>

<style scoped src="../settings-local.css"></style>
<style scoped>
.take-error { color: var(--danger, var(--warning)); }
</style>
