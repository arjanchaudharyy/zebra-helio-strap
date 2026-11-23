import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen as tauriListen, type EventCallback, type UnlistenFn } from '@tauri-apps/api/event';

export interface DemoIpc {
  __zbDemo: true;
  invoke(command: string, args?: Record<string, unknown>): Promise<unknown>;
  transformCallback(callback: (event: { event: string; id: number; payload: unknown }) => void): number;
}

declare global {
  interface Window { __ZB_IPC__?: DemoIpc }
}

const demoIpc = (): DemoIpc | undefined => (typeof window === 'undefined' ? undefined : window.__ZB_IPC__);

export const demoIpcActive = (): boolean => demoIpc()?.__zbDemo === true;

export const invoke = <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
  const demo = demoIpc();
  return demo ? (demo.invoke(command, args) as Promise<T>) : tauriInvoke<T>(command, args);
};

export const listen = async <T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> => {
  const demo = demoIpc();
  if (!demo) return tauriListen<T>(event, handler);
  const handlerId = demo.transformCallback(handler as (event: { event: string; id: number; payload: unknown }) => void);
  const eventId = await demo.invoke('plugin:event|listen', { event, target: { kind: 'Any' }, handler: handlerId });
  return () => { void demo.invoke('plugin:event|unlisten', { event, eventId }).catch(() => undefined); };
};
