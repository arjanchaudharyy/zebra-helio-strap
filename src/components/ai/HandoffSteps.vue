<script setup lang="ts">
import Icon, { type IconName } from '../Icon.vue';
import type { HandoffStep, HandoffStepId, HandoffStepState } from '../../composables/useAiTaskHandoff';
import { defineMessages, useMessages } from '../../i18n';

defineProps<{ steps: Record<HandoffStepId, HandoffStep>; providerLabel: string }>();
const emit = defineEmits<{ (event: 'retry', id: HandoffStepId): void }>();

const t = useMessages(defineMessages(
  {
    prepare: 'Prepare the file',
    copy: 'Copy the opening line',
    open: (label: string) => `Open ${label}`,
    idle: 'Not started', doing: 'Working', done: 'Done', failed: 'Failed', blocked: 'Blocked', skipped: 'Web preview cannot open a browser',
    retry: 'Retry',
  },
));

const ORDER: HandoffStepId[] = ['prepare', 'copy', 'open'];
const ICON: Record<HandoffStepState, IconName> = {
  idle: 'ring', doing: 'refresh', done: 'circle-check', failed: 'warning', blocked: 'warning', skipped: 'info',
};
</script>

<template>
  <ol class="steps">
    <li v-for="id in ORDER" :key="id" :class="['step', `is-${steps[id].state}`]">
      <Icon :name="ICON[steps[id].state]" :size="15" :class="{ spinning: steps[id].state === 'doing' }" />
      <span class="step-copy">
        <span class="step-name">{{ id === 'open' ? t.open(providerLabel) : t[id] }}</span>
        <span class="step-state">{{ steps[id].errorText || t[steps[id].state] }}</span>
      </span>
      <button v-if="id !== 'prepare' && steps[id].state === 'failed'" type="button" class="ai-tool" @click="emit('retry', id)">
        {{ t.retry }}
      </button>
    </li>
  </ol>
</template>

<style scoped>
.steps { display: grid; gap: 6px; margin: 12px 0 0; padding: 0; list-style: none; }
.step { display: flex; align-items: center; gap: 10px; color: var(--subtle); }
.step.is-done { color: var(--accent); }
.step.is-failed, .step.is-blocked { color: var(--danger); }
.step.is-doing { color: var(--ink); }
.step-copy { display: grid; flex: 1; min-width: 0; }
.step-name { color: var(--ink); font-size: var(--fs-sm); }
.step-state { font-size: var(--fs-xs); }
.spinning { animation: spin 1s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
@media (prefers-reduced-motion: reduce) { .spinning { animation: none; } }
</style>
