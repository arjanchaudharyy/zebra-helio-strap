import { computed, ref, watch } from 'vue';
import { backend, toUserMessage } from '../../lib/bridge';
import { useMessages } from '../../i18n';
import { errorTextFor } from '../../i18n/errors';
import { backendText } from '../../i18n/backendText';
import { credentialRecoveryMessages } from '../../components/CredentialRecovery.i18n';
import { settingsMessages } from '../../views/Settings.i18n';
import type { AccountPair, OfficialStatus } from '../../types';
import type { SettingsFeedback } from './useSettingsFeedback';
import { applyOfficialStatus } from '../sync/run';

const idle = (): OfficialStatus => ({
  state: 'idle', message_code: null, message: null, user_id_masked: null, nickname: null, connected_at: null,
  authorize_url: null,
});

export const createOfficialAuth = (feedback: SettingsFeedback) => {
  const t = useMessages(settingsMessages);
  const recoveryText = useMessages(credentialRecoveryMessages);
  const status = ref<OfficialStatus>(idle());
  const busy = ref(false);
  const failureText = ref<string | null>(null);
  const failureCode = ref<string | null>(null);
  const failureConflict = ref<AccountPair | null>(null);
  watch(failureText, (value) => {
    if (!value) {
      failureCode.value = null;
      failureConflict.value = null;
    }
  });
  const accountConflict = computed(() => (
    failureCode.value === 'err.official.other_account' || failureCode.value === 'err.core.account_mismatch'
  ));
  const waitingText = ref<string | null>(null);
  const checking = ref(false);
  let unlisten: (() => void) | undefined;
  let attachGen = 0;
  let copiedTimer: number | undefined;
  const CALLBACK_SLOW_MS = 45_000;
  const callbackSlow = ref(false);
  let slowTimer: ReturnType<typeof setTimeout> | undefined;
  const trackWaiting = (waitingNow: boolean) => {
    if (!waitingNow) {
      clearTimeout(slowTimer);
      slowTimer = undefined;
      callbackSlow.value = false;
    } else if (slowTimer === undefined && !callbackSlow.value) {
      slowTimer = setTimeout(() => { callbackSlow.value = true; }, CALLBACK_SLOW_MS);
    }
  };

  const waiting = computed(() => status.value.state === 'waiting');
  const browserTrouble = computed(() => waiting.value && Boolean(status.value.authorize_url) && (
    status.value.message_code === 'err.official.browser'
    || status.value.message_code === 'err.official.browser_not_opened'
  ));
  const connected = computed(() => status.value.state === 'connected');
  const needsReauth = computed(() => status.value.state === 'needs_reauth');

  const apply = (next: OfficialStatus) => {
    status.value = next;
    trackWaiting(next.state === 'waiting');
    applyOfficialStatus(next);
    waitingText.value = next.state === 'waiting' && next.message_code ? errorTextFor(next.message_code) ?? null : null;
    if (next.state !== 'waiting' || next.message_code) checking.value = false;
    if (next.state === 'failed') {
      failureText.value = errorTextFor(next.message_code ?? undefined)
        ?? backendText(next.message ?? '', t.value.officialFailed);
      failureCode.value = next.message_code ?? null;
      failureConflict.value = next.conflict ?? null;
    } else if (next.state !== 'idle') {
      failureText.value = null;
    }
  };

  const start = async () => {
    failureText.value = null;
    busy.value = true;
    try {
      apply(await backend.startOfficialLogin());
    } catch (cause) {
      failureText.value = toUserMessage(cause, t.value.officialFailed);
    } finally {
      busy.value = false;
    }
  };

  const cancel = async () => {
    busy.value = true;
    try {
      apply(await backend.cancelOfficialLogin());
      failureText.value = null;
    } catch (cause) {
      failureText.value = toUserMessage(cause, t.value.officialFailed);
    } finally {
      busy.value = false;
    }
  };

  const recover = async (storage: 'system' | 'file') => {
    busy.value = true;
    try {
      const next = await backend.recoverOfficialCredentials(storage);
      apply(next);
      if (next.state === 'connected' && storage === 'file') {
        feedback.dataMessage.value = recoveryText.value.savedFile;
      }
    } catch (cause) {
      failureText.value = toUserMessage(cause, t.value.officialFailed);
    } finally {
      busy.value = false;
    }
  };

  const checkNow = async () => {
    checking.value = true;
    try {
      await backend.checkOfficialLoginNow();
      window.setTimeout(() => { checking.value = false; }, 45_000);
    } catch (cause) {
      checking.value = false;
      failureText.value = toUserMessage(cause, t.value.officialFailed);
    }
  };

  const disconnect = async () => {
    if (!window.confirm(t.value.officialDisconnectConfirm)) return;
    busy.value = true;
    try {
      apply(await backend.disconnectOfficial());
      feedback.dataMessage.value = t.value.officialDisconnected;
    } catch (cause) {
      failureText.value = toUserMessage(cause, t.value.officialFailed);
    } finally {
      busy.value = false;
    }
  };

  const linkCopied = ref(false);
  const copyLink = async () => {
    const url = status.value.authorize_url;
    if (!url) return;
    try {
      await navigator.clipboard.writeText(url);
      linkCopied.value = true;
      window.clearTimeout(copiedTimer);
      copiedTimer = window.setTimeout(() => { linkCopied.value = false; }, 2400);
    } catch (cause) {
      failureText.value = toUserMessage(cause, t.value.officialFailed);
    }
  };

  const attach = async () => {
    detach();
    const gen = attachGen;
    try {
      const off = await backend.listen<OfficialStatus>('official://status', apply);
      if (gen !== attachGen) { off(); return; }
      unlisten = off;
      const current = await backend.getOfficialStatus();
      if (gen === attachGen) apply(current);
    } catch {
    }
  };
  const detach = () => {
    attachGen += 1;
    unlisten?.();
    unlisten = undefined;
    clearTimeout(copiedTimer);
    trackWaiting(false);
  };

  return {
    status, busy, failureText, accountConflict, failureConflict, waitingText, checking, waiting, browserTrouble, callbackSlow, connected, needsReauth, linkCopied,
    start, cancel, recover, checkNow, disconnect, copyLink, attach, detach,
  };
};

export type OfficialAuth = ReturnType<typeof createOfficialAuth>;
