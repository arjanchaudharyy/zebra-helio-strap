import { createApp, type Component } from 'vue';
import { createMemoryHistory, createRouter } from 'vue-router';
import GlassWall from './GlassWall.vue';
import CardWall from './CardWall.vue';
import DeviceWall from './DeviceWall.vue';
import MetricWall from './MetricWall.vue';
import type { FilmWall } from '../presentation';

const WALLS: Record<FilmWall, Component> = { glass: GlassWall, cards: CardWall, devices: DeviceWall, metrics: MetricWall };

export const mountWall = (kind: FilmWall): { unmount(): void } => {
  const host = document.createElement('div');
  host.className = 'film-wall-host';
  document.body.append(host);
  const app = createApp(WALLS[kind]);
  app.use(createRouter({ history: createMemoryHistory(), routes: [{ path: '/:any(.*)*', component: { render: () => null } }] }));
  app.mount(host);
  return { unmount() { app.unmount(); host.remove(); } };
};
