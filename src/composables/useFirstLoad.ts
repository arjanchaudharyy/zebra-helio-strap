import { computed, ref, watch, type Ref } from 'vue';
import { whenMotionIdle } from '../lib/motion/budget';

export const useFirstLoad = (loading: Ref<boolean>) => {
  const settled = ref(!loading.value);
  watch(loading, (value) => {
    if (!value && !settled.value) void whenMotionIdle().then(() => { settled.value = true; });
  });
  return computed(() => !settled.value);
};

export const useLoadingAfterMotion = (loading: Ref<boolean>): Readonly<Ref<boolean>> => {
  const shown = ref(loading.value);
  watch(loading, (value) => {
    if (value) {
      shown.value = true;
      return;
    }
    void whenMotionIdle().then(() => { if (!loading.value) shown.value = false; });
  });
  return shown;
};
