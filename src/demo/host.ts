export const SITE_SOURCE = 'zeppbridge-site';
export const DEMO_SOURCE = 'zeppbridge-demo';

export const demoRequested = (): boolean => typeof window !== 'undefined' && window.__ZB_DEMO__?.demo === true;

export const hostPost = (type: string, payload: Record<string, unknown> = {}): void => {
  try {
    if (window.parent && window.parent !== window) {
      window.parent.postMessage({ source: DEMO_SOURCE, type, ...payload }, window.location.origin);
    }
  } catch { }
};

export interface SiteMessage { type: 'go' | 'theme' | 'locale' | 'scroll' | 'visibility'; to?: string; value?: string; selector?: string | null }

export const listenToSite = (handle: (message: SiteMessage) => void): (() => void) => {
  const onMessage = (event: MessageEvent) => {
    if (event.origin !== window.location.origin || event.source !== window.parent) return;
    const data = event.data as { source?: string } & SiteMessage;
    if (data?.source !== SITE_SOURCE) return;
    handle(data);
  };
  window.addEventListener('message', onMessage);
  return () => window.removeEventListener('message', onMessage);
};
