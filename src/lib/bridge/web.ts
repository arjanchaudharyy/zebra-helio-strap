import { DesktopUnavailableError } from './errors';
import type { BridgeBackend, UnlistenFn } from './types';

const unavailable = (): Promise<never> => Promise.reject(new DesktopUnavailableError());
const noopListen = (): Promise<UnlistenFn> => Promise.resolve(() => undefined);

export const webBackend = new Proxy({} as BridgeBackend, {
  get: (_target, key) => (key === 'listen' ? noopListen : key === 'then' ? undefined : unavailable),
});
