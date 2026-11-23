<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue';
import { useTrainingPlan } from '../../../composables/useTrainingPlan';
import { useBridgeText } from './bridge.i18n';
import { usePlanText } from '../../plan/usePlanText';
import { isDesktop } from '../../../lib/bridge';
import { createPaper, type Paper } from '../../../lib/motion/paper';
import Icon from '../../Icon.vue';

const props = withDefaults(defineProps<{ variant?: 'icon' | 'primary' | 'pill'; target?: () => HTMLElement | null }>(), { variant: 'pill', target: undefined });
const emit = defineEmits<{ received: [] }>();
const t = useBridgeText();
const { t: pt } = usePlanText();
const plan = useTrainingPlan();
const button = ref<HTMLButtonElement | null>(null);
const webOpen = ref(false);
const reply = ref('');
const running = ref(false);
let paper: Paper | null = null;

const reason = (): string => {
  const n = plan.notice.value;
  if (n?.kind === 'clipboard_not_a_plan') return t.value.receiveEmpty;
  if (n?.kind === 'paste_failed') return { empty: pt.value.pasteEmpty, no_json: pt.value.pasteNoJson, not_a_plan: pt.value.pasteNotPlan }[n.failure];
  if (n?.kind === 'error' && n.text) return n.text;
  return pt.value.noticeError;
};

const run = async (read: () => Promise<string | null>) => {
  if (running.value || plan.busy.value || !button.value) return;
  running.value = true;
  paper = createPaper(button.value);
  try {
    const pulled = paper.pullOut();
    const text = await read();
    await pulled;
    if (!text) { await paper.fail(reason()); return; }
    const lines = text.split(/\r?\n/);
    paper.fill(lines.filter((line) => line.trim()).slice(0, 5), t.value.transcript(lines.length));
    if (await paper.land(props.target?.() ?? null, () => plan.paste(text), reason)) emit('received');
  } finally {
    paper = null;
    running.value = false;
  }
};

const receive = () => {
  if (!isDesktop()) { webOpen.value = !webOpen.value; return; }
  void run(() => plan.readClipboardPlan());
};
const paste = () => {
  const text = reply.value;
  if (!text.trim()) return;
  reply.value = '';
  webOpen.value = false;
  void run(async () => text);
};
onBeforeUnmount(() => paper?.dispose());
</script>

<template>
  <div :class="['receive', `is-${variant}`]">
    <button ref="button" type="button" class="receive-button" :disabled="plan.busy.value || running" :aria-label="t.receive" :title="variant === 'icon' ? t.receive : undefined" @click="receive">
      <Icon name="copy" :size="variant === 'primary' ? 18 : 15" />
      <span v-if="variant !== 'icon'">{{ running ? t.receiving : t.receive }}</span>
    </button>
    <div v-if="webOpen" class="web-paste">
      <label>{{ t.previewPaste }}<textarea v-model="reply" rows="4"></textarea></label>
      <div class="web-acts">
        <button type="button" class="pill-button quiet" @click="webOpen = false">{{ t.close }}</button>
        <button type="button" class="pill-button" :disabled="!reply.trim() || plan.busy.value" @click="paste">{{ t.inspect }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped src="./ReceiveCapsule.css"></style>
