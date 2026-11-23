export const settleCssAnimations = (root: Element | null | undefined): void => {
  if (!root?.isConnected || typeof CSSAnimation === 'undefined' || typeof root.getAnimations !== 'function') return;
  for (const animation of root.getAnimations({ subtree: true })) {
    if (animation instanceof CSSAnimation && Number.isFinite(animation.effect?.getTiming().iterations ?? 1)) animation.finish();
  }
};
