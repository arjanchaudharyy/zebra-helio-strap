<script setup lang="ts">
import { computed, ref } from 'vue';
import { useRouter } from 'vue-router';
import CredentialRecovery from '../../../components/CredentialRecovery.vue';
import Icon from '../../../components/Icon.vue';
import FoldTransition from '../../../components/FoldTransition.vue';
import ModalDialog from '../../../components/ModalDialog.vue';
import WhyHalfData from '../../../components/WhyHalfData.vue';
import { whyHalfDataMessages } from '../../../components/WhyHalfData.i18n';
import { useSettingsContext } from '../../../composables/settings/context';
import { useSyncController } from '../../../composables/useSyncController';
import { revealWeeklyReport } from '../../../lib/revealWeeklyReport';
import { useMessages } from '../../../i18n';
import { openFaq } from '../../../lib/faqLink';
import { settingsMessages } from '../../Settings.i18n';
import { deckMessages } from '../deck.i18n';
import { accountCardMessages } from './account.i18n';
import { workoutToolsMessages } from '../../../components/WorkoutTools.i18n';

const d = useMessages(deckMessages);
const a = useMessages(accountCardMessages);
const w = useMessages(whyHalfDataMessages);
const t = useMessages(settingsMessages);
const tools = useMessages(workoutToolsMessages);
const whyOpen = ref(false);
const { appStatus, isSyncing, syncProgress, dataReady, pickUpReady } = useSyncController();
const router = useRouter();

const firstSyncing = computed(() => isSyncing.value && !appStatus.value?.last_cloud_sync_at);
const firstReady = computed(() => dataReady.value.phase === 'ready' && dataReady.value.firstRun && !isSyncing.value);
const firstLine = computed(() => {
  if (firstReady.value) return d.value.firstReady;
  const progress = syncProgress.value;
  return progress ? d.value.firstSyncing(progress.current, progress.total) : d.value.firstSyncingPlain;
});
const goOverview = () => {
  if (firstReady.value) pickUpReady('pickup');
  void router.push('/').then(() => revealWeeklyReport());
};

const {
  connected, configuredOnly, needsReauth, accountRecognized, loginBusy, loginInProgress, loginStatus, loginMessage,
  startLogin, cancelLogin, recoverCredentials, manualAuthBusy, verifyAndSync, clearAuth,
} = useSettingsContext().auth;
const official = useSettingsContext().official;

const name = computed(() =>
  official.status.value.nickname?.trim() || official.status.value.user_id_masked
  || appStatus.value?.masked_user_id || a.value.accountFallback);
const initial = computed(() => name.value.match(/[\p{L}\p{N}]/u)?.[0]?.toUpperCase() || 'Z');

const officialState = computed(() => {
  if (official.waiting.value) return a.value.stWaiting;
  if (official.connected.value) return a.value.stOn;
  return official.needsReauth.value ? a.value.stReauth : a.value.stOff;
});
const advancedState = computed(() => {
  if (loginInProgress.value) return a.value.stSigningIn;
  if (connected.value) return a.value.stOn;
  if (needsReauth.value) return a.value.stReauth;
  if (configuredOnly.value) return a.value.stUnverified;
  return loginStatus.value.state === 'failed' ? a.value.stFailed : a.value.stOff;
});
const statusLine = computed(() => [a.value.official(officialState.value), a.value.advanced(advancedState.value)].join(' · '));
const anyConnected = computed(() => official.connected.value || accountRecognized.value);

type Action = { label: string; primary: boolean; disabled: boolean; run: () => unknown };
const action = computed<Action | null>(() => {
  if (official.waiting.value) return { label: a.value.cancel, primary: false, disabled: official.busy.value, run: official.cancel };
  if (!official.connected.value) {
    return { label: official.needsReauth.value ? a.value.reauth : a.value.connect, primary: true, disabled: official.busy.value, run: official.start };
  }
  if (loginInProgress.value) return { label: a.value.cancel, primary: false, disabled: loginBusy.value, run: cancelLogin };
  if (configuredOnly.value) return { label: a.value.verify, primary: false, disabled: isSyncing.value, run: verifyAndSync };
  if (!connected.value) return { label: a.value.connectAdvanced, primary: false, disabled: loginBusy.value, run: startLogin };
  return null;
});
</script>

<template>
  <section id="account-section" class="s-section" aria-labelledby="account-title">
    <div class="s-section-head"><h3 id="account-title">{{ d.secAccount }}</h3></div>
    <div class="s-list">
      <div v-if="firstSyncing || firstReady" class="s-row first-run" role="status">
        <span :class="['first-dot', { done: firstReady }]" aria-hidden="true"></span>
        <div class="s-row-main"><span class="s-row-title">{{ firstLine }}</span></div>
        <div class="s-row-control">
          <button :class="['button', firstReady ? 'primary' : 'secondary']" type="button" @click="goOverview">{{ d.goOverview }}</button>
        </div>
      </div>
      <div class="s-row">
        <span class="account-avatar" :class="{ muted: !anyConnected }" aria-hidden="true">{{ initial }}</span>
        <div class="s-row-main">
          <span class="s-row-title">{{ name }}</span>
          <span class="s-row-sub">{{ statusLine }}</span>
        </div>
        <div v-if="action" class="s-row-control">
          <button :class="['button', action.primary ? 'primary' : 'secondary']" type="button" :disabled="action.disabled" @click="action.run">
            {{ action.label }}
          </button>
        </div>
      </div>
      <div v-if="official.waiting.value" class="s-row">
        <div class="s-row-main"><span class="s-row-sub">{{ a.waitingHint }}</span><button type="button" class="why-link" @click="openFaq('relay')">{{ t.officialWhy }}</button></div>
        <div class="s-row-control">
          <button v-if="official.browserTrouble.value" class="button primary" type="button" @click="official.copyLink">
            <Icon :name="official.linkCopied.value ? 'check' : 'link'" :size="14" />{{ official.linkCopied.value ? t.linkCopied : t.copyAuthLink }}
          </button>
          <button class="button secondary" type="button" :disabled="official.checking.value" @click="official.checkNow">{{ a.checkNow }}</button>
        </div>
      </div>
      <div class="s-row">
        <div class="s-row-main">
          <span class="s-row-title">{{ w.title }}</span>
          <span class="s-row-sub">{{ a.whySub }}</span>
        </div>
        <div class="s-row-control">
          <button class="pill-button" type="button" @click="whyOpen = true">{{ a.learnMore }}</button>
        </div>
      </div>
      <div v-if="appStatus?.auth_configuration_present || appStatus?.configured" class="s-row">
        <div class="s-row-main">
          <span class="s-row-title">{{ a.logoutTitle }}</span>
          <span class="s-row-sub">{{ a.logoutSub }}</span>
        </div>
        <div class="s-row-control">
          <button class="button danger-button" type="button" @click="clearAuth">{{ a.logoutShort }}</button>
        </div>
      </div>
      <p v-if="appStatus?.auth_configuration_present && !appStatus?.credential_available" class="s-note" role="status">{{ tools.credentialHint }}</p>
    </div>
    <FoldTransition><p v-if="official.failureText.value" class="api-error" role="alert"><Icon name="info" :size="13" />{{ official.failureText.value }}</p></FoldTransition>
    <CredentialRecovery v-if="official.status.value.credential_recovery"
      :recovery="official.status.value.credential_recovery" :busy="official.busy.value"
      @retry="official.recover" @discard="official.cancel" />
    <CredentialRecovery v-if="loginStatus.credential_recovery" kind="advanced"
      :recovery="loginStatus.credential_recovery" :busy="loginBusy || manualAuthBusy"
      @retry="recoverCredentials" @discard="cancelLogin" />
    <FoldTransition><p v-if="official.waiting.value && official.waitingText.value" class="hint-line" role="status"><Icon name="info" :size="13" />{{ official.waitingText.value }}</p></FoldTransition>
    <FoldTransition><p v-if="official.waiting.value && official.callbackSlow.value && !official.browserTrouble.value" class="hint-line" role="status"><Icon name="info" :size="13" />{{ t.officialCallbackSlow }}</p></FoldTransition>
    <FoldTransition>
      <p v-if="loginInProgress && loginMessage" class="hint-line"><Icon name="info" :size="13" />{{ loginMessage }}</p>
      <p v-else-if="loginStatus.state === 'failed' && loginMessage" class="api-error" role="alert"><Icon name="info" :size="13" />{{ loginMessage }}</p>
    </FoldTransition>
    <ModalDialog v-if="whyOpen" labelledby="why-half-title" @close="whyOpen = false">
      <div class="why-dialog">
        <WhyHalfData title-id="why-half-title" />
        <footer><button class="button primary" type="button" @click="whyOpen = false">{{ w.close }}</button></footer>
      </div>
    </ModalDialog>
  </section>
</template>

<style scoped src="../settings-local.css"></style>
<style scoped>
.first-run { background: color-mix(in srgb, var(--accent) 7%, transparent); }
.first-dot { flex: none; width: 9px; height: 9px; margin: 0 14px 0 13px; border-radius: 50%; background: var(--accent); animation: first-breathe 1.6s ease-in-out infinite; }
.first-dot.done { animation: none; }
@keyframes first-breathe { 50% { opacity: .35; } }
@media (prefers-reduced-motion: reduce) { .first-dot { animation: none; } }
.account-avatar {
  display: grid;
  width: 36px;
  height: 36px;
  flex: 0 0 36px;
  place-items: center;
  border-radius: 11px;
  background: var(--accent-soft);
  color: var(--accent);
  font-family: var(--font-display, inherit);
  font-size: var(--fs-lg);
  font-weight: 700;
}
.why-dialog { padding: 20px; }
.why-dialog footer { display: flex; justify-content: flex-end; margin-top: 24px; }
.why-dialog footer .button { min-height: 44px; min-width: 110px; }
.account-avatar.muted { background: var(--mat-inset); box-shadow: var(--mat-inset-shadow); color: var(--subtle); }
</style>
