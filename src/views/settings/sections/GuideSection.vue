<script setup lang="ts">
import { ref } from 'vue';
import Icon from '../../../components/Icon.vue';
import { launchSandbox } from '../../../composables/useTour';
import { ALL_CHAPTERS, INTRO, SECTIONS, type TourChapterId } from '../../../lib/tour/chapters';
import { forgetSeen, seenChapters } from '../../../lib/tour/sandbox';
import { useTourText } from '../../../components/tour/tour.i18n';

const t = useTourText();
const seen = ref(seenChapters());
const resetDone = ref(false);

const look = (chapter: TourChapterId) => launchSandbox([chapter], { ret: '/settings/guide' });
const resetAll = () => {
  forgetSeen([...ALL_CHAPTERS]);
  seen.value = seenChapters();
  resetDone.value = true;
};
</script>

<template>
  <section class="s-section">
    <div class="s-list">
      <div class="s-row">
        <div class="s-row-main">
          <span class="s-row-title">{{ t.chapter[INTRO] }}</span>
          <span v-if="seen.has(INTRO)" class="s-row-sub guide-seen"><Icon name="circle-check" :size="13" />{{ t.seenMark }}</span>
        </div>
        <div class="s-row-control">
          <button type="button" class="button primary" @click="look(INTRO)">{{ t.start }}</button>
        </div>
      </div>
      <div v-for="id in SECTIONS" :key="id" class="s-row">
        <div class="s-row-main">
          <span class="s-row-title">{{ t.chapter[id] }}</span>
          <span v-if="seen.has(id)" class="s-row-sub guide-seen"><Icon name="circle-check" :size="13" />{{ t.seenMark }}</span>
        </div>
        <div class="s-row-control">
          <button type="button" class="button secondary" @click="look(id)">{{ t.look }}</button>
        </div>
      </div>
      <div class="s-row">
        <div class="s-row-main">
          <span class="s-row-title">{{ t.resetAll }}</span>
          <span v-if="resetDone" class="s-row-sub" role="status">{{ t.resetAllDone }}</span>
        </div>
        <div class="s-row-control">
          <button type="button" class="button secondary" :disabled="resetDone" :aria-label="t.resetAll" :title="t.resetAll" @click="resetAll"><Icon name="refresh" :size="14" /></button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.guide-seen { display: inline-flex; align-items: center; gap: 4px; }
</style>
