import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router';
import { installReturnScroll } from '../lib/returnScroll';

/* Every page is a dynamic import, so a cold start loads only the page on screen. */
const routes: RouteRecordRaw[] = [
  { path: '/', name: 'Overview', component: () => import('../v4/views/Today.vue') },
  { path: '/insights', name: 'Insights', component: () => import('../v4/views/Insights.vue') },
  { path: '/live', name: 'Live', component: () => import('../v4/views/Live.vue') },
  { path: '/recovery', name: 'Recovery', component: () => import('../v4/views/Recovery.vue') },
  { path: '/sleep', name: 'SleepList', component: () => import('../v4/views/Sleep.vue') },
  { path: '/sleep/:sleepId', name: 'SleepDetail', component: () => import('../views/SleepDetail.vue') },
  { path: '/heart', name: 'HeartRateDetail', component: () => import('../v4/views/Heart.vue') },
  { path: '/activity', name: 'ActivityDetail', component: () => import('../v4/views/Activity.vue') },
  { path: '/workouts', name: 'WorkoutList', component: () => import('../v4/views/Workouts.vue') },
  { path: '/workouts/:workoutId', name: 'WorkoutDetail', component: () => import('../views/WorkoutDetail.vue') },
  { path: '/age', name: 'BodyAge', component: () => import('../v4/views/Age.vue') },
  { path: '/coach', name: 'Coach', component: () => import('../v4/views/Coach.vue') },
  { path: '/settings/:card?', name: 'Settings', component: () => import('../v4/views/Settings.vue') },
  { path: '/:pathMatch(.*)*', redirect: { path: '/' } },
];

const router = createRouter({
  history: createWebHistory(),
  routes,
  scrollBehavior: () => ({ top: 0 }),
});

// The scroll area is #main-content: forward goes to the top, back returns to where you were.
installReturnScroll(router);

export default router;
