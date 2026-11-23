import { computed, ref, watch } from 'vue';
import { deviceStateLabel, useDevices } from '../useDevices';
import { useSyncController } from '../useSyncController';
import { backend, errorCode, toUserMessage } from '../../lib/bridge';
import { locale, useMessages } from '../../i18n';
import { errorTextFor } from '../../i18n/errors';
import { backendText } from '../../i18n/backendText';
import { credentialRecoveryMessages } from '../../components/CredentialRecovery.i18n';
import { settingsMessages } from '../../views/Settings.i18n';
import type { AccountPair, LoginStatus } from '../../types';
import type { SettingsFeedback } from './useSettingsFeedback';

const DEFAULT_REGION_HOST = 'https://api-mifit-us3.zepp.com';
const idleStatus = (): LoginStatus => ({ state: 'idle', message: '', page_url: '' });

export const createAuthFlow = (feedback: SettingsFeedback) => {
  const t = useMessages(settingsMessages);
  const recoveryText = useMessages(credentialRecoveryMessages);
  const { appStatus, refreshStatus, runSync, isSyncing } = useSyncController();
  const { load: loadDevices } = useDevices();
  const { dataMessage, dataError } = feedback;

  const reconnecting = ref(false);
  const loginStatus = ref<LoginStatus>(idleStatus());
  const loginError = ref<string | null>(null);
  const loginBusy = ref(false);
  const loginErrorCode = ref<string | undefined>();
  const loginConflict = ref<AccountPair | null>(null);
  watch(loginError, (value) => {
    if (!value) {
      loginErrorCode.value = undefined;
      loginConflict.value = null;
    }
  });
  const accountConflict = computed(() => (
    loginErrorCode.value === 'err.login.other_account' || loginErrorCode.value === 'err.core.account_mismatch'
  ));
  const freshStartBusy = ref(false);
  let unlistenLogin: (() => void) | undefined;
  let attachGen = 0;

  const showManualAuth = ref(false);
  const manualAppToken = ref('');
  const manualUserId = ref('');
  const manualRegionHost = ref(DEFAULT_REGION_HOST);
  const manualAuthBusy = ref(false);

  const resetManualInput = () => {
    showManualAuth.value = false;
    manualAppToken.value = '';
    manualUserId.value = '';
    manualRegionHost.value = DEFAULT_REGION_HOST;
  };

  const credentialAvailable = computed(() => appStatus.value?.credential_available ?? Boolean(appStatus.value?.configured));
  const configurationPresent = computed(() => appStatus.value?.auth_configuration_present ?? Boolean(appStatus.value?.configured));
  const connected = computed(() => credentialAvailable.value && appStatus.value?.auth_state === 'verified');
  const needsReauth = computed(() => configurationPresent.value && (!credentialAvailable.value || appStatus.value?.auth_state === 'needs_reauth'));
  const configuredOnly = computed(() => credentialAvailable.value && !connected.value && !needsReauth.value);
  const accountRecognized = computed(() => connected.value || configuredOnly.value);
  const loginInProgress = computed(() => ['waiting', 'extracting', 'verifying'].includes(String(loginStatus.value.state)));

  const loginMessage = computed(() => {
    const status = loginStatus.value;
    if (!status.message && !status.code) return '';
    if (status.code === 'err.login.timeout') return recoveryText.value.advancedExpired;
    return errorTextFor(status.code) ?? backendText(status.message, '');
  });

  const connectionLabel = computed(() => {
    if (loginInProgress.value) {
      if (loginStatus.value.state === 'extracting') return t.value.connExtracting;
      if (loginStatus.value.state === 'verifying') return t.value.connVerifying;
      return t.value.connWaiting;
    }
    if (loginStatus.value.state === 'failed' && !accountRecognized.value) return t.value.connFailed;
    if (connected.value || configuredOnly.value) return deviceStateLabel('account');
    return deviceStateLabel('unknown');
  });

  const applyLoginStatus = async (status: LoginStatus, opts?: { replay?: boolean }) => {
    const justConnected = status.state === 'connected' && loginStatus.value.state !== 'connected';
    loginStatus.value = status;
    if (status.credential_recovery) resetManualInput();
    if (status.state === 'configured') {
      reconnecting.value = false;
      loginError.value = null;
      resetManualInput();
      await refreshStatus();
      dataMessage.value = t.value.manualAuthDone;
    }
    if (status.state === 'connected') {
      reconnecting.value = false;
      loginError.value = null;
      await refreshStatus();
      if (justConnected && !opts?.replay) {
        if (isSyncing.value) await runSync('incremental', undefined, { silent: true });
        void runSync('incremental', undefined, { silent: true, waited: true, skipProbe: true });
        void loadDevices(true);
      }
    }
    if (status.state === 'failed') {
      loginError.value = errorTextFor(status.code)
        ?? backendText(status.message, t.value.loginIncomplete);
      loginErrorCode.value = status.code ?? undefined;
      loginConflict.value = status.conflict ?? null;
    }
  };

  const startLogin = async () => {
    loginError.value = null;
    loginBusy.value = true;
    reconnecting.value = true;
    try {
      await applyLoginStatus(await backend.startWebLogin(locale.value));
    } catch (error) {
      loginStatus.value = { state: 'failed', message: toUserMessage(error, t.value.loginWindowFailed), page_url: '' };
      loginError.value = toUserMessage(error, t.value.loginWindowFailed);
    } finally {
      loginBusy.value = false;
    }
  };

  const cancelLogin = async () => {
    loginBusy.value = true;
    try {
      await applyLoginStatus(await backend.cancelWebLogin());
      reconnecting.value = false;
      loginError.value = null;
      resetManualInput();
    } catch (error) {
      loginError.value = toUserMessage(error, t.value.loginCancelFailed);
    } finally {
      loginBusy.value = false;
    }
  };

  const submitManualAuth = async () => {
    if (!manualAppToken.value || !manualUserId.value || !manualRegionHost.value) {
      loginError.value = t.value.fillAllFields;
      return;
    }
    manualAuthBusy.value = true;
    loginError.value = null;
    try {
      await backend.manualAuth(
        manualAppToken.value.trim(),
        manualUserId.value.trim(),
        manualRegionHost.value.trim(),
      );
      await refreshStatus();
      loginStatus.value = idleStatus();
      reconnecting.value = false;
      showManualAuth.value = false;
      manualAppToken.value = '';
      manualUserId.value = '';
      manualRegionHost.value = DEFAULT_REGION_HOST;
      dataMessage.value = t.value.manualAuthDone;
    } catch (error) {
      loginError.value = toUserMessage(error, t.value.manualAuthFailed);
      loginErrorCode.value = errorCode(error);
      try {
        const current = await backend.getLoginStatus();
        if (current.credential_recovery) await applyLoginStatus(current, { replay: true });
      } catch { /* Keep the original save error if status is unavailable. */ }
    } finally {
      manualAuthBusy.value = false;
    }
  };

  const recoverCredentials = async (storage: 'system' | 'file') => {
    if (loginBusy.value || manualAuthBusy.value) return;
    loginBusy.value = true;
    try {
      const next = await backend.recoverAdvancedCredentials(storage);
      await applyLoginStatus(next);
      if (['connected', 'configured'].includes(next.state) && storage === 'file') {
        dataMessage.value = recoveryText.value.savedFile;
      }
    } catch (error) {
      loginError.value = toUserMessage(error, t.value.manualAuthFailed);
    } finally {
      loginBusy.value = false;
    }
  };

  const verifyAndSync = async () => {
    try {
      await backend.verifyAuth();
      await refreshStatus();
      await runSync('incremental');
    } catch (error) {
      dataError.value = toUserMessage(error, t.value.verifyFailed);
    }
  };

  const startFresh = async () => {
    if (!window.confirm(t.value.freshStartConfirm)) return;
    freshStartBusy.value = true;
    try {
      await backend.startFreshLibrary();
    } catch (error) {
      dataError.value = toUserMessage(error, t.value.clearAuthFailed);
      freshStartBusy.value = false;
    }
  };

  const clearAuth = async () => {
    if (!window.confirm(t.value.logoutConfirm)) return;
    dataError.value = null;
    try {
      await backend.clearAuth();
      await refreshStatus();
      reconnecting.value = false;
      loginStatus.value = idleStatus();
      resetManualInput();
      dataMessage.value = t.value.authCleared;
    } catch (error) {
      dataError.value = toUserMessage(error, t.value.clearAuthFailed);
    }
  };

  const attach = async () => {
    detach();
    const gen = attachGen;
    try {
      const off = await backend.listen<LoginStatus>('login://status', (payload) => { void applyLoginStatus(payload); });
      if (gen !== attachGen) { off(); return; }
      unlistenLogin = off;
      const current = await backend.getLoginStatus();
      if (gen === attachGen) await applyLoginStatus(current, { replay: true });
    } catch {
      // Browser preview has no login IPC.
    }
  };
  const detach = () => {
    attachGen += 1;
    unlistenLogin?.();
    unlistenLogin = undefined;
  };

  return {
    reconnecting,
    loginStatus,
    loginError,
    accountConflict,
    loginConflict,
    freshStartBusy,
    startFresh,
    loginBusy,
    showManualAuth,
    manualAppToken,
    manualUserId,
    manualRegionHost,
    manualAuthBusy,
    connected,
    needsReauth,
    configuredOnly,
    accountRecognized,
    loginInProgress,
    loginMessage,
    connectionLabel,
    startLogin,
    cancelLogin,
    submitManualAuth,
    recoverCredentials,
    verifyAndSync,
    clearAuth,
    attach,
    detach,
  };
};

export type AuthFlow = ReturnType<typeof createAuthFlow>;
