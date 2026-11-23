<script setup lang="ts">
import { computed, ref } from 'vue';
import Icon from '../../Icon.vue';
import GlassSwitch from '../../GlassSwitch.vue';
import HandoffSteps from '../HandoffSteps.vue';
import ReceiveCapsule from '../bridge/ReceiveCapsule.vue';
import { isDesktop } from '../../../lib/bridge';
import { startFileDrag } from '../../../lib/dragOut';
import { formatBytes } from '../../../lib/format';
import { formatTokens } from '../../../lib/aiTask/budget';
import type { AiTaskPrepareResult } from '../../../lib/bridge/types';
import type { HandoffStep, HandoffStepId } from '../../../composables/useAiTaskHandoff';
import { useHandoffText } from '../HandoffDock.i18n';
import { useStageText } from './stage.i18n';
import { useHandoffLead } from './useHandoffLead';
import { autoPasteOn, autoSupported, setAutoPaste } from '../../../composables/ai/useAutoHandoff';

const props = defineProps<{
  ready: AiTaskPrepareResult | null; provider: string; providerIcon: string; stale: boolean; recalled: boolean;
  steps: Record<HandoffStepId, HandoffStep>; saveError: string | null; question: string;
}>();
const emit = defineEmits<{ copy: []; open: []; reveal: []; retry: [HandoffStepId]; edit: []; received: [] }>();
const t = useHandoffText();
const s = useStageText();
const desktop = isDesktop();
const fileName = computed(() => props.ready?.md_path?.split(/[\\/]/).pop() ?? '');
const dragFailed = ref(false);
const failed = computed(() => Object.values(props.steps).some((step) => step.state === 'failed'));
const note = computed(() => (props.recalled ? s.value.notOpened : props.stale ? t.value.stale : dragFailed.value ? t.value.dragFailed : props.saveError));
const lead = useHandoffLead(() => props.provider);
const TIP_KEY = 'zeppbridge.ai.autoTipSeen';
const tipSeen = () => { try { return window.localStorage.getItem(TIP_KEY) === '1'; } catch { return true; } };
const showAutoTip = ref(autoSupported() && !autoPasteOn.value && !tipSeen());
if (showAutoTip.value) { try { window.localStorage.setItem(TIP_KEY, '1'); } catch { } }
const onFileDrag = (event: MouseEvent) => {
  const path = props.ready?.md_path;
  if (!desktop || !path || event.button !== 0 || (event.target as HTMLElement).closest('.reveal')) return;
  dragFailed.value = false;
  startFileDrag(path, fileName.value).catch(() => { dragFailed.value = true; });
};
</script>

<template>
  <div class="receipt">
    <header data-enter="head">
      <span class="logo"><img :src="providerIcon" alt="" /></span>
      <div class="head-copy">
        <h3>{{ s.receiptTitle(provider) }}</h3>
        <p v-if="question">{{ question }}</p>
      </div>
      <button type="button" class="edit" @click="emit('edit')"><Icon name="edit" :size="12" />{{ s.editQuestion }}</button>
    </header>

    <div v-if="ready?.md_path" :class="['sheet', { draggable: desktop }]" data-enter="file" role="button" tabindex="0" :aria-label="t.fileAria(fileName)" :title="s.receiptLead"
      @mousedown="onFileDrag" @keydown.enter.prevent="emit('reveal')">
      <button type="button" class="reveal" :title="t.revealFile" :aria-label="t.revealFile" @click.stop="emit('reveal')"><Icon name="dots" :size="16" /></button>
      <span class="sheet-lines" aria-hidden="true"><i v-for="n in 5" :key="n"></i></span>
      <span class="sheet-tag" aria-hidden="true">.md</span>
      <span class="sheet-copy"><strong>{{ fileName }}</strong><small>{{ t.fileMeta(formatBytes(ready.byte_len), formatTokens(ready.markdown?.approx_tokens ?? 0)) }}</small></span>
    </div>
    <p v-else-if="ready" class="hint" data-enter="file">{{ t.outputAt(ready.output_dir) }}</p>
    <p class="hint lead" role="status" data-enter="rest">{{ lead }}</p>
    <div v-if="showAutoTip" class="auto-tip" data-enter="rest">
      <span>{{ s.autoTip }}</span>
      <GlassSwitch :model-value="autoPasteOn" :aria-label="s.autoTip" @update:model-value="setAutoPaste" />
      <button type="button" class="tip-close" :title="s.autoTipClose" :aria-label="s.autoTipClose" @click="showAutoTip = false"><Icon name="x" :size="12" /></button>
    </div>
    <p v-if="note" class="note" role="status" data-enter="rest">{{ note }}</p>
    <HandoffSteps v-if="failed" class="steps" data-enter="rest" :steps="steps" :provider-label="provider" @retry="emit('retry', $event)" />

    <div class="actions" data-enter="act">
      <button type="button" class="pill-button quiet" @click="emit('copy')">{{ s.copyAgain }}</button>
      <button type="button" class="pill-button quiet" @click="emit('open')">{{ s.openAgain(provider) }}</button>
    </div>
    <ReceiveCapsule class="receive-row" data-enter="act" variant="primary" @received="emit('received')" />
  </div>
</template>

<style scoped>
.receipt { display: flex; flex-direction: column; gap: 14px; height: 100%; min-height: 0; }
header { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: 12px; align-items: start; }
.logo { display: grid; place-items: center; width: 40px; height: 40px; border-radius: 12px; background: var(--mat-raised); box-shadow: var(--mat-raised-rim); }
.logo img { width: 22px; height: 22px; border-radius: 5px; object-fit: contain; }
.head-copy { min-width: 0; padding-top: 1px; }
h3 { margin: 0; font-size: var(--fs-md); font-weight: 700; line-height: 1.3; }
.head-copy p { display: -webkit-box; margin: 4px 0 0; overflow: hidden; color: var(--muted); font-size: var(--fs-xs); line-height: 1.45; -webkit-line-clamp: 2; -webkit-box-orient: vertical; }
.edit { display: inline-flex; align-items: center; gap: 4px; padding: 4px 10px; border: 1px solid var(--mat-glass-line); border-radius: 999px;
  background: color-mix(in srgb, var(--ink) 5%, transparent); color: var(--muted); font-size: var(--fs-2xs); font-weight: 600; cursor: pointer; white-space: nowrap;
  transition: background var(--dur-base) ease, color var(--dur-base) ease; }
.edit:hover { background: color-mix(in srgb, var(--ink) 10%, transparent); color: var(--ink); }
.edit:focus-visible, .reveal:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; border-radius: 6px; }
.sheet { position: relative; display: grid; flex: 1 1 auto; grid-template-rows: minmax(0, 1fr) auto auto; gap: 10px; min-height: 108px; overflow: hidden; padding: 16px 18px 14px;
  border: 1px solid color-mix(in srgb, var(--ink) 7%, transparent); border-radius: 14px 22px 14px 14px;
  background: linear-gradient(180deg, color-mix(in srgb, var(--ink) 3%, var(--mat-inset)), var(--mat-inset)); box-shadow: inset 0 1px 0 color-mix(in srgb, var(--ink) 5%, transparent); }
.sheet::after { content: ''; position: absolute; top: -1px; right: -1px; width: 22px; height: 22px; border-radius: 0 22px 0 8px; background: var(--mat-raised); box-shadow: var(--mat-raised-rim); }
.sheet.draggable { cursor: grab; }
.sheet.draggable:active { cursor: grabbing; }
.sheet:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
.sheet-lines { display: grid; align-content: start; gap: 8px; min-height: 0; overflow: hidden; padding-top: 6px; }
.sheet-lines i { height: 4px; border-radius: 2px; background: color-mix(in srgb, var(--ink) 7%, transparent); }
.sheet-lines i:nth-child(2) { width: 82%; }
.sheet-lines i:nth-child(4) { width: 64%; }
.sheet-lines i:nth-child(5) { width: 40%; }
.sheet-tag { justify-self: start; padding: 2px 8px; border-radius: 6px; background: var(--mat-raised); box-shadow: var(--mat-raised-rim); color: var(--muted); font: 700 10px var(--font-mono); }
.sheet-copy { display: grid; gap: 2px; min-width: 0; padding-right: 30px; }
.sheet-copy strong { overflow: hidden; font-size: var(--fs-xs); text-overflow: ellipsis; white-space: nowrap; }
.sheet-copy small { color: var(--subtle); font-size: var(--fs-2xs); }
.reveal { position: absolute; z-index: 1; right: 8px; bottom: 12px; display: grid; place-items: center; width: 30px; height: 30px; padding: 0; border: 0; border-radius: 50%; background: none; color: var(--subtle); cursor: pointer; }
.reveal:hover { background: var(--glass-press); color: var(--ink); }
.hint { flex: 0 0 auto; margin: -4px 0 0; color: var(--subtle); font-size: var(--fs-2xs); line-height: 1.5; }
.auto-tip { display: flex; flex: 0 0 auto; align-items: center; gap: 8px; margin: -6px 0 0; padding: 6px 6px 6px 12px; border-radius: 12px; background: var(--mat-inset); box-shadow: var(--mat-inset-shadow); }
.auto-tip span { flex: 1 1 auto; min-width: 0; color: var(--muted); font-size: var(--fs-2xs); line-height: 1.45; }
.tip-close { display: grid; flex: 0 0 auto; place-items: center; width: 24px; height: 24px; padding: 0; border: 0; border-radius: 50%; background: none; color: var(--subtle); cursor: pointer; }
.tip-close:hover { background: var(--glass-press); color: var(--ink); }
.tip-close:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
.note { margin: -6px 0 0; color: var(--warning); font-size: var(--fs-2xs); line-height: 1.5; }
.steps { font-size: var(--fs-2xs); }
.actions { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
.actions .pill-button { justify-content: center; min-width: 0; overflow: hidden; border: 1px solid var(--mat-glass-line); background: color-mix(in srgb, var(--ink) 5%, transparent);
  font-size: var(--fs-2xs); text-overflow: ellipsis; white-space: nowrap; }
.actions .pill-button:hover { background: color-mix(in srgb, var(--ink) 10%, transparent); }
.receive-row { display: grid; }
.receive-row :deep(.receive-button) { width: 100%; }
</style>
