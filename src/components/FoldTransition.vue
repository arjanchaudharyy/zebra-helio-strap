<script setup lang="ts">
import { lessMotion } from '../lib/motion/lessMotion';
const props = withDefaults(defineProps<{
  duration?: number;
  origin?: string;
  scale?: number;
}>(), { duration: 320, origin: '50% 0', scale: 1 });
const from = () => `translateY(-6px) scale(${props.scale})`;
const EASE = 'cubic-bezier(.4, .6, .2, 1)';
const reduced = () => lessMotion();

const enter = (el: Element, done: () => void) => {
  const node = el as HTMLElement;
  if (reduced()) { done(); return; }
  const height = node.scrollHeight;
  node.style.overflow = 'clip';
  node.style.transformOrigin = props.origin;
  const animation = node.animate(
    [
      { height: '0px', opacity: 0, transform: from(), marginTop: '0px', marginBottom: '0px' },
      { height: `${height}px`, opacity: 1, transform: 'none' },
    ],
    { duration: props.duration, easing: EASE },
  );
  const finish = () => { node.style.overflow = ''; node.style.transformOrigin = ''; done(); };
  animation.finished.then(finish, finish);
};

const leave = (el: Element, done: () => void) => {
  const node = el as HTMLElement;
  if (reduced()) { done(); return; }
  const height = node.offsetHeight;
  node.style.overflow = 'clip';
  node.style.transformOrigin = props.origin;
  const animation = node.animate(
    [
      { height: `${height}px`, opacity: 1, transform: 'none' },
      { height: '0px', opacity: 0, transform: from(), marginTop: '0px', marginBottom: '0px' },
    ],
    { duration: Math.round(props.duration * 0.8), easing: 'cubic-bezier(.4, 0, .7, .2)', fill: 'forwards' },
  );
  animation.finished.then(done, done);
};
</script>

<template>
  <Transition :css="false" @enter="enter" @leave="leave"><slot /></Transition>
</template>
