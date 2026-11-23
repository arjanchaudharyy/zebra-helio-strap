import { computed, getCurrentInstance, onActivated, onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue';
import { createLensFilter, lensEnabled, lensSupported, type LensFilter, type LensKind } from '../lib/glassLens';
import { afterMotion } from '../lib/motion/budget';

const whenIdle = (run: () => void) => {
  if (typeof window.requestIdleCallback === 'function') window.requestIdleCallback(run, { timeout: 400 });
  else window.setTimeout(run, 16);
};

const isolatedFromPage = (from: HTMLElement | null): boolean => {
  for (let node = from; node && node !== document.body; node = node.parentElement) {
    const style = getComputedStyle(node);
    if (style.backdropFilter !== 'none' && style.backdropFilter !== '') return true;
    if (style.filter !== 'none') return true;
    if (style.maskImage !== 'none' && style.maskImage !== '') return true;
  }
  return false;
};

export const useGlassLens = (target: Ref<HTMLElement | null>, kind: LensKind = 'thumb', when = true) => {
  const instance = getCurrentInstance();
  const filterRef = ref<string | null>(null);
  let lens: LensFilter | null = null;
  let observer: ResizeObserver | null = null;

  const fit = (el: HTMLElement) => lens?.resize(el.offsetWidth, el.offsetHeight);

  let disposed = false;
  let waiting = false;
  let stopWatch: (() => void) | null = null;
  const attach = () => {
    if (disposed || lens) return;
    const root = instance?.proxy?.$el as unknown;
    const from = target.value?.parentElement ?? (root instanceof HTMLElement ? root : null);
    if (!from?.isConnected) {
      waiting = true;
      return;
    }
    if (isolatedFromPage(from)) return;
    lens = createLensFilter(kind);
    filterRef.value = lens.ref;
    observer = new ResizeObserver((entries) => {
      for (const entry of entries) fit(entry.target as HTMLElement);
    });
    stopWatch = watch(target, (el, previous) => {
      if (previous) observer?.unobserve(previous);
      if (el) {
        observer?.observe(el);
        fit(el);
      }
    }, { immediate: true });
  };
  onMounted(() => {
    if (!when || !lensEnabled.value || !lensSupported()) return;
    afterMotion(() => whenIdle(attach));
  });
  onActivated(() => {
    if (!waiting) return;
    waiting = false;
    afterMotion(() => whenIdle(attach));
  });

  onBeforeUnmount(() => {
    disposed = true;
    stopWatch?.();
    observer?.disconnect();
    lens?.dispose();
    lens = null;
  });

  const prime = () => {
    if (lens || disposed || !when || !lensEnabled.value || !lensSupported()) return;
    waiting = false;
    attach();
  };

  const active = computed(() => filterRef.value !== null);
  const style = (extra = '') => {
    if (!filterRef.value) return {};
    const value = `${extra} ${filterRef.value}`.trim();
    return { backdropFilter: value, WebkitBackdropFilter: value };
  };

  return { active, style, prime };
};
