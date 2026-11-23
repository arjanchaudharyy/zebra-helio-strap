/// <reference types="vite/client" />

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<{}, {}, any>;
  export default component;
}

declare module "*.svg?raw" {
  const source: string;
  export default source;
}

declare const __BUILD_STAMP__: string;

/** True only for the Microsoft Store package build. */
declare const __STORE_BUILD__: boolean;
