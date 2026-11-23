import { computed, inject, onBeforeUnmount, provide, shallowReactive, watch, type ComputedRef, type InjectionKey } from 'vue';
import { groupState, RESTING, type GlassPose, type GroupMember, type GroupState } from '../lib/glassGroup';
import type { LensRect } from '../lib/segmentGlass';

export interface GlassGroup {
  members: Map<string, GroupMember>;
  state: ComputedRef<GroupState>;
}

const KEY: InjectionKey<GlassGroup> = Symbol('glass-group');
let serial = 0;

export const provideGlassGroup = (reach = 24): GlassGroup => {
  const members = shallowReactive(new Map<string, GroupMember>());
  const state = computed(() => groupState([...members.values()], reach));
  const group: GlassGroup = { members, state };
  provide(KEY, group);
  return group;
};

export const useGlassGroup = (): GlassGroup | null => inject(KEY, null);

const toRect = (box: DOMRect): LensRect => ({ x: box.left, y: box.top, w: box.width, h: box.height });

export const useGlassMember = (
  name: string,
  element: () => HTMLElement | null,
  pose: () => GlassPose,
): GlassGroup | null => {
  const group = useGlassGroup();
  if (!group) return null;
  const id = `${name}-${(serial += 1)}`;
  const report = (current: GlassPose) => {
    const el = element();
    group.members.set(id, { id, rect: el?.isConnected ? toRect(el.getBoundingClientRect()) : null, pose: current });
  };
  const stop = watch(pose, report, { immediate: true, flush: 'post' });
  onBeforeUnmount(() => {
    stop();
    group.members.delete(id);
  });
  return group;
};

export { RESTING };
