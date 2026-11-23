<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, type CSSProperties } from 'vue';
import { transformOf, type Placement } from '../../../lib/stage/layout';
import { HANDLES, type Handle } from '../../../lib/stage/gestures';
import { useMessages } from '../../../i18n';
import { stageMessages } from './stage.i18n';

const props = defineProps<{ id: string; title: string; placement: Placement; menuOpen: boolean }>();
const emit = defineEmits<{
  grab: [event: PointerEvent, kind: 'move' | 'resize', handle?: Handle];
  activate: [];
  menu: [anchor: HTMLElement];
  measure: [height: number];
}>();
const t = useMessages(stageMessages);
const content = ref<HTMLElement | null>(null);
const live = computed(() => props.placement.mode === 'stage' || props.placement.mode === 'pip' || props.placement.mode === 'classic');
const grip = computed(() => props.placement.mode === 'stage' || props.placement.mode === 'pip');
const style = computed((): CSSProperties => ({
  width: `${props.placement.w}px`,
  '--win-h': `${props.placement.h}px`,
  transform: transformOf(props.placement),
  opacity: String(props.placement.opacity),
  zIndex: String(props.placement.z),
  visibility: props.placement.opacity === 0 && props.placement.mode !== 'strip' ? 'hidden' : undefined,
  ...(props.placement.extra ? { '--fill-extra': `${props.placement.extra}px` } : {}),
}));

let observer: ResizeObserver | null = null;
onMounted(() => {
  observer = new ResizeObserver(() => {
    if (content.value) emit('measure', Math.ceil(content.value.offsetHeight - (props.placement.extra ?? 0)));
  });
  if (content.value) observer.observe(content.value);
});
onBeforeUnmount(() => observer?.disconnect());
</script>

<template>
  <div
    class="stage-window"
    :class="[`mode-${placement.mode}`, { 'menu-open': menuOpen, cropped: placement.crop, fill: placement.fill }]"
    :style="style"
    :data-stage-card="id"
    role="region"
    :aria-label="title"
    :aria-hidden="placement.mode === 'hidden' || placement.mode === 'behind' ? 'true' : undefined"
  >
    <div class="window-body" data-stage-scroll :inert="!live || undefined" :style="placement.pad ? { paddingBottom: `${placement.pad}px` } : undefined">
      <div ref="content" class="window-content"><slot /></div>
    </div>
    <template v-if="grip">
      <button
        class="window-grip"
        type="button"
        :aria-label="t.menu(title)"
        aria-haspopup="menu"
        :aria-expanded="menuOpen"
        @pointerdown="emit('grab', $event, 'move')"
        @click="emit('menu', $event.currentTarget as HTMLElement)"
      ><i></i><i></i><i></i></button>
      <template v-if="placement.mode === 'stage' && placement.sides">
        <span
          v-for="handle in HANDLES"
          :key="handle"
          class="window-handle"
          :class="`handle-${handle}`"
          :title="t.resize(title)"
          aria-hidden="true"
          @pointerdown="emit('grab', $event, 'resize', handle)"
        ></span>
      </template>
    </template>
    <button
      v-else-if="placement.mode === 'strip'"
      class="window-hit"
      type="button"
      :aria-label="t.openCard(title)"
      :title="title"
      :tabindex="placement.opacity === 0 ? -1 : undefined"
      @pointerdown="emit('grab', $event, 'move')"
      @click="emit('activate')"
    ></button>
  </div>
</template>

<style scoped src="./StageWindow.css"></style>
