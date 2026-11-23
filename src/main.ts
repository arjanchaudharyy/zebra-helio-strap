import "./demo/early";
import { createApp } from "vue";
import App from "./App.vue";
import router from "./router";
import { initializeLocale } from "./i18n";
import { initializeTheme } from "./composables/useTheme";
import { initializeEffects } from "./composables/useEffects";
import { prepareUpdateNotice } from "./services/updateNotice";
import "./styles/fonts.css";
import "./styles/tokens.css";
import "./styles/material.css";


initializeTheme();
initializeEffects();

const demo = window.__ZB_DEMO__?.demo === true;
// Updates from upstream are off in this build: they would replace the redesign.
void prepareUpdateNotice;
const tourSandbox = demo && window.__ZB_DEMO__?.tour !== undefined;
const demoRuntimeInstalled = () => window.__ZB_IPC__?.__zbDemo === true;
const abandonSandbox = () => {
  void import("./lib/tour/sandbox").then(({ reloadUrl }) => window.location.replace(reloadUrl(window.__ZB_DEMO__?.ret)),
    () => window.location.replace('/'));
};

const mount = () => {
  const app = createApp(App);
  app.use(router).mount("#app");
  if (!demo && '__TAURI_INTERNALS__' in window) void import("./lib/bridge").then(async ({ backend }) => {
    if ((await backend.getUserPrefs()).demo_mode) await router.replace('/');
  }).catch(() => undefined);
  if (demo && !tourSandbox) void import("./demo/bootstrap").then((module) => module.startDemoHost(router));
};

let mounted = false;
const mountOnce = () => {
  if (mounted) return;
  mounted = true;
  mount();
};
const ready = demo
  ? import("./demo/runtime")
    .then((module) => module.installDemoRuntime())
    .then(() => import("./demo/bootstrap"))
    .then((module) => module.prepareDemoAppearance())
  : Promise.resolve();
if (tourSandbox) {
  void ready.then(() => {
    if (!demoRuntimeInstalled()) { abandonSandbox(); return; }
    void initializeLocale().then(mountOnce, mountOnce);
  }, abandonSandbox);
} else {
  void ready.then(() => initializeLocale()).then(mountOnce, mountOnce);
  window.setTimeout(mountOnce, demo ? 6000 : 800);
}

const syncBackgrounded = () => {
  document.documentElement.classList.toggle("is-backgrounded", document.hidden);
};
document.addEventListener("visibilitychange", syncBackgrounded);
syncBackgrounded();
