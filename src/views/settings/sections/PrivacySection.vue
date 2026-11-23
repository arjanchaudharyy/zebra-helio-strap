<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { useRouter } from 'vue-router';
import Icon from '../../../components/Icon.vue';
import ModalDialog from '../../../components/ModalDialog.vue';
import { useLocalApi } from '../../../composables/settings/useLocalApi';
import { useMessages } from '../../../i18n';
import { settingsMessages } from '../../Settings.i18n';
import { privacyCardMessages } from './privacy.i18n';

const t = useMessages(settingsMessages);
const p = useMessages(privacyCardMessages);
const privacyModalOpen = ref(false);
const router = useRouter();
const { localApiStatus, loadLocalApiStatus } = useLocalApi();
const openLocalApi = () => { void router.push({ path: '/settings/advanced', hash: '#local-api' }); };

const STALE_PRIVACY_PREF_KEYS = [
  'zeppbridge-pref-encrypt',
  'zeppbridge-pref-launch-lock',
  'zeppbridge-pref-anon',
];
onMounted(() => {
  for (const key of STALE_PRIVACY_PREF_KEYS) window.localStorage.removeItem(key);
  void loadLocalApiStatus();
});
</script>

<template>
  <section id="privacy-section" class="s-section">
    <div class="s-list">
      <div class="s-row">
        <div class="s-row-main"><span class="s-row-title">{{ p.lead }}</span></div>
        <div class="s-row-control">
          <button class="pill-button quiet" type="button" @click="privacyModalOpen = true">
            <Icon name="shield" :size="14" />{{ p.learnMore }}
          </button>
        </div>
      </div>
      <div class="s-row">
        <div class="s-row-main">
          <span class="s-row-title">{{ t.localApiLabel }}</span>
          <span v-if="localApiStatus" class="s-row-sub">{{ localApiStatus.enabled ? p.apiOn(localApiStatus.base_url || '127.0.0.1') : p.apiOff }}</span>
        </div>
        <div class="s-row-control">
          <button class="pill-button" type="button" @click="openLocalApi">{{ p.apiOpen }}<Icon name="chevron-right" :size="14" /></button>
        </div>
      </div>
    </div>

    <ModalDialog v-if="privacyModalOpen" labelledby="privacy-dialog-title" @close="privacyModalOpen = false">
      <div class="modal-head">
        <div class="modal-title-row">
          <Icon name="shield" :size="18" class="shield-ic" />
          <h3 id="privacy-dialog-title">{{ t.privacyModalTitle }}</h3>
        </div>
        <button type="button" class="close-btn" :aria-label="t.closeDialog" @click="privacyModalOpen = false"><Icon name="x" :size="16" /></button>
      </div>
      <div class="modal-body">
        <p><strong>{{ t.privacyDbTitle }}</strong>{{ t.privacyDbBody }}</p>
        <p><strong>{{ t.privacyTokenTitle }}</strong>{{ t.privacyTokenBody }}</p>
        <p><strong>{{ t.privacyTelemetryTitle }}</strong>{{ t.privacyTelemetryBody }}</p>
        <p><strong>{{ t.privacyPoint1Title }}</strong>{{ t.privacyPoint1 }}</p>
        <p><strong>{{ t.privacyPoint2Title }}</strong>{{ t.privacyPoint2 }}</p>
        <p><strong>{{ t.privacyPoint3Title }}</strong>{{ t.privacyPoint3 }}</p>
        <p><strong>{{ t.privacyPoint4Title }}</strong>{{ t.privacyPoint4 }}</p>
        <p><strong>{{ t.privacyPoint5Title }}</strong>{{ t.privacyPoint5 }}</p>
      </div>
      <div class="modal-foot">
        <button type="button" class="button primary" @click="privacyModalOpen = false">{{ t.privacyModalOk }}</button>
      </div>
    </ModalDialog>
  </section>
</template>

<style scoped src="../settings-local.css"></style>
<style scoped src="../settings-modal.css"></style>
