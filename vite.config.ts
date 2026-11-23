import { execSync } from "node:child_process";
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

const buildStamp = () => {
  // @ts-expect-error process is a nodejs global
  const injected: string | undefined = process.env.ZEPPBRIDGE_BUILD_SHA;
  let sha = "unknown";
  if (injected && injected.trim()) {
    //
    sha = injected.trim().slice(0, 7);
  } else {
    try {
      sha = execSync("git rev-parse --short HEAD", { encoding: "utf8" }).trim();
    } catch {
    }
  }
  const now = new Date();
  const pad = (value: number) => String(value).padStart(2, "0");
  const at = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`
    + ` ${pad(now.getHours())}:${pad(now.getMinutes())}`;
  return `${sha} · ${at}`;
};

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async ({ mode }) => ({
  plugins: [vue()],
  publicDir: false,

  // Tauri serves the bundled frontend from its asset protocol rather than
  // from an HTTP origin. Relative URLs keep imported WebP/PNG assets inside
  // the bundle (and still work from Vite's dev server) instead of resolving
  // to the host/root path where WebView2 reports a broken image. Keep every
  // icon as a local emitted file rather than a data URL, because the desktop
  // CSP intentionally does not allow arbitrary inline image payloads.
  base: './',
  define: {
    __BUILD_STAMP__: JSON.stringify(buildStamp()),
    __STORE_BUILD__: JSON.stringify(process.env.ZB_STORE_BUILD === "1"),
  },
  build: {
    outDir: mode === 'site' ? '.site-cache/site-dist' : 'dist',
    //
    //
    //
    target: ['es2020', 'edge88', 'firefox78', 'chrome87', 'safari14'],
    assetsInlineLimit: 0,
    rollupOptions: {
      output: {
        entryFileNames: 'assets/[name]-[hash].js',
        chunkFileNames: 'assets/[name]-[hash].js',
        manualChunks(id: string) {
          if (!id.includes('node_modules')) return undefined;
          if (id.includes('echarts') || id.includes('zrender')) return 'charts';
          if (id.includes('/leaflet/')) return 'map';
          if (id.includes('/three/') || id.includes('/postprocessing/')) return 'particles';
          if (id.includes('/vue/') || id.includes('vue-router') || id.includes('@vue/')) {
            return 'vue';
          }
          return 'vendor';
        },
      },
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
