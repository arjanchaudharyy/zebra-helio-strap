import type { Router } from 'vue-router';
import { setTheme } from '../composables/useTheme';
import { hostPost, listenToSite } from './host';
import { installPresentation } from './presentation';

export const prepareDemoAppearance = (): void => {
  const flags = window.__ZB_DEMO__;
  if (flags?.theme === 'light' || flags?.theme === 'dark') setTheme(flags.theme);
};

const scrollMain = (selector: string | null): void => {
  const main = document.querySelector<HTMLElement>('.main-content');
  if (!main) return;
  const target = selector ? document.querySelector<HTMLElement>(selector) : null;
  const top = target ? main.scrollTop + target.getBoundingClientRect().top - main.getBoundingClientRect().top - 12 : 0;
  main.scrollTo({ top: Math.max(0, top), behavior: 'smooth' });
};

export const startDemoHost = (router: Router): void => {
  if (window.__ZB_DEMO__?.showcase) installPresentation(router);
  listenToSite((message) => {
    if (message.type === 'go' && message.to && message.to.startsWith('/') && !message.to.startsWith('//')) {
      void router.push(message.to);
    } else if (message.type === 'scroll') {
      scrollMain(message.selector ?? null);
    } else if (message.type === 'theme' && (message.value === 'light' || message.value === 'dark')) {
      setTheme(message.value);
    } else if (message.type === 'visibility') {
      document.documentElement.classList.toggle('is-backgrounded', message.value === 'hidden');
    }
  });
  router.afterEach((to) => hostPost('route', { path: to.path }));
  hostPost('ready', { path: router.currentRoute.value.path });
};
