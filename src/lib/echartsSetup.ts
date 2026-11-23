import { registerTheme, throttle, use } from 'echarts/core';
import { BarChart, LineChart, PieChart } from 'echarts/charts';
import {
  GridComponent,
  LegendComponent,
  MarkAreaComponent,
  MarkLineComponent,
  MarkPointComponent,
  TooltipComponent,
  VisualMapPiecewiseComponent,
} from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import VChartBase from 'vue-echarts';
import { computed, defineComponent, h, onBeforeUnmount, ref, shallowRef, type Component, type VNode } from 'vue';
import { resolvedTheme } from '../composables/useTheme';
import { chartPalettes, zeppThemeDark, zeppThemeLight } from './echartsTheme';
import { deferResize, motionBusy, nextSlot, whenMotionIdle } from './motion/budget';

use([
  LineChart,
  BarChart,
  PieChart,
  GridComponent,
  TooltipComponent,
  LegendComponent,
  MarkLineComponent,
  MarkAreaComponent,
  MarkPointComponent,
  VisualMapPiecewiseComponent,
  CanvasRenderer,
]);
registerTheme('zeppbridge-dark', zeppThemeDark);
registerTheme('zeppbridge-light', zeppThemeLight);


const RESIZE_THROTTLE_MS = 100;
type ChartHandle = {
  chart?: { getWidth: () => number; getHeight: () => number; resize: () => void; dispatchAction: (action: { type: string }) => void };
  root?: HTMLElement;
};

const followSize = (handle: () => ChartHandle | null): (() => void) => {
  const host = handle()?.root?.firstElementChild ?? handle()?.root;
  if (!host || typeof ResizeObserver === 'undefined') return () => undefined;
  const apply = () => {
    const chart = handle()?.chart;
    const width = host.clientWidth;
    const height = host.clientHeight;
    if (!chart || !width || !height || (width === chart.getWidth() && height === chart.getHeight())) return;
    chart.dispatchAction({ type: 'hideTip' });
    chart.resize();
  };
  const resize = throttle(() => {
    if (!deferResize(apply)) apply();
  }, RESIZE_THROTTLE_MS);
  const observer = new ResizeObserver(() => resize());
  observer.observe(host);
  return () => {
    observer.disconnect();
    resize.clear();
  };
};

const DeferredChart = defineComponent({
  name: 'DeferredChart',
  inheritAttrs: false,
  setup(_, { attrs, slots }) {
    const ready = ref(!motionBusy.value);
    const born = performance.now();
    let alive = true;
    let late = false;
    if (!ready.value) {
      void whenMotionIdle().then(() => nextSlot(() => {
        if (!alive) return;
        late = performance.now() - born > 160;
        ready.value = true;
      }));
    }
    const chart = shallowRef<ChartHandle | null>(null);
    let stopFollowing = () => {};
    onBeforeUnmount(() => {
      alive = false;
      stopFollowing();
    });
    const fadeIn = (vnode: VNode) => {
      const el = vnode.el as HTMLElement | null;
      el?.animate?.([{ opacity: 0 }, { opacity: 1 }], { duration: 300, easing: 'ease-out' });
    };
    const onMounted = (vnode: VNode) => {
      if (late) fadeIn(vnode);
      stopFollowing();
      if (autoresize()) stopFollowing = followSize(() => chart.value);
    };
    const onUnmounted = () => {
      stopFollowing();
      stopFollowing = () => {};
    };
    const autoresize = () => 'autoresize' in attrs && attrs.autoresize !== false;
    return () => {
      if (!ready.value) return h('div', { class: attrs.class, style: attrs.style, 'aria-hidden': 'true' });
      const { autoresize: _autoresize, ...rest } = attrs;
      return h(
        VChartBase as Component,
        { ...rest, ref: chart, onVnodeMounted: onMounted, onVnodeUnmounted: onUnmounted },
        slots,
      );
    };
  },
});

export const VChart = DeferredChart as unknown as typeof VChartBase;

export const CHART_THEME = computed(() =>
  resolvedTheme.value === 'light' ? 'zeppbridge-light' : 'zeppbridge-dark');

export const chartPalette = computed(() => chartPalettes[resolvedTheme.value]);
