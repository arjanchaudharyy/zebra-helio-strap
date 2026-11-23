import { isTauriRuntime, tauriBackend } from './tauri';
import { webBackend } from './web';
import type { BridgeBackend } from './types';

export type { BridgeBackend, UnlistenFn } from './types';
export { backendLate, tauriBackend, whenBackendReady } from './tauri';
export { webBackend } from './web';
export {
  DesktopUnavailableError,
  TauriUnavailableError,
  errorCode,
  toUserMessage,
} from './errors';

export const isDesktop = isTauriRuntime;
export const isTauri = isDesktop;

export const getBackend = (): BridgeBackend => (isDesktop() ? tauriBackend : webBackend);

export const backend: BridgeBackend = new Proxy({} as BridgeBackend, {
  get: (_target, key) => (getBackend() as unknown as Record<string | symbol, unknown>)[key],
});
