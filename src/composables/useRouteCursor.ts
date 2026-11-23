import { readonly, ref } from 'vue';

const cursor = ref<number | null>(null);

export const routeCursor = readonly(cursor);

export const setRouteCursor = (time: number | null): void => {
  const next = time !== null && Number.isFinite(time) ? time : null;
  if (cursor.value !== next) cursor.value = next;
};

export const axisPointerTime = (event: { axesInfo?: Array<{ axisDim?: string; value?: unknown }> } | null | undefined): number | null => {
  const info = event?.axesInfo?.find((item) => item.axisDim === 'x');
  return typeof info?.value === 'number' ? info.value : null;
};
