<script setup lang="ts">
import GlassSwitch from '../../../components/GlassSwitch.vue';
import { useServerPush } from '../../../composables/useServerPush';
import { useMessages } from '../../../i18n';
import { labFlag, setLabFlag } from '../../../lib/labs';
import { cloudMessages } from './cloud.i18n';
import { labsMessages } from './labs.i18n';
import PushSection from './PushSection.vue';

const t = useMessages(labsMessages);
const c = useMessages(cloudMessages);
const lab = labFlag('cloud');
const push = useServerPush();

const toggle = async (on: boolean) => {
  setLabFlag('cloud', on);
  await push.refresh();
  if (push.state.value?.configured && push.state.value.enabled !== on) await push.setEnabled(on);
};
</script>

<template>
  <section id="labs" class="s-section">
    <p class="s-note">{{ t.lead }}</p>
    <div class="s-list">
      <div class="s-row">
        <div class="s-row-main">
          <span class="s-row-title">{{ c.labTitle }} <span class="lab-badge">{{ t.badge }}</span></span>
          <span class="s-row-sub">{{ c.labSub }}</span>
        </div>
        <div class="s-row-control">
          <GlassSwitch data-setting="labs-cloud" :model-value="lab" :aria-label="c.labTitle" :disabled="!!push.busy.value"
            @update:model-value="toggle" />
        </div>
      </div>
    </div>
  </section>
  <PushSection v-if="lab" />
</template>

<style scoped src="../settings-local.css"></style>
<style scoped src="./labs.css"></style>
