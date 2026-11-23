/* Liquid segmented controls: one glass thumb per `.v4-tabs` that slides to the selected option
   with a spring and stretches like liquid while it moves. Works for every switch in the app,
   because it watches the DOM for `aria-pressed` changes. Respects reduced motion. */
const reduce = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches;

const place = (group: HTMLElement, animate: boolean) => {
  let thumb = group.querySelector<HTMLElement>(':scope > .v4-tab-thumb');
  if (!thumb) {
    thumb = document.createElement('span');
    thumb.className = 'v4-tab-thumb';
    thumb.setAttribute('aria-hidden', 'true');
    group.prepend(thumb);
  }
  const on = group.querySelector<HTMLElement>(':scope > button[aria-pressed="true"]');
  if (!on) { thumb.style.opacity = '0'; return; }
  const x = on.offsetLeft;
  const w = on.offsetWidth;
  const prevX = Number(thumb.dataset.x ?? x);
  const prevW = Number(thumb.dataset.w ?? w);
  thumb.dataset.x = String(x);
  thumb.dataset.w = String(w);
  thumb.style.opacity = '1';
  thumb.style.width = `${w}px`;
  thumb.style.transform = `translateX(${x}px)`;
  if (!animate || reduce() || (prevX === x && prevW === w)) return;
  // Liquid move: stretch toward the target, overshoot slightly, settle (spring-like).
  const dist = Math.abs(x - prevX);
  const stretch = 1 + Math.min(0.35, dist / 300);
  thumb.animate(
    [
      { transform: `translateX(${prevX}px) scaleX(1)`, width: `${prevW}px` },
      { transform: `translateX(${prevX + (x - prevX) * 0.55}px) scaleX(${stretch}) scaleY(0.9)`, width: `${(prevW + w) / 2}px`, offset: 0.45 },
      { transform: `translateX(${x + (x - prevX) * 0.04}px) scaleX(0.97) scaleY(1.03)`, width: `${w}px`, offset: 0.8 },
      { transform: `translateX(${x}px) scaleX(1) scaleY(1)`, width: `${w}px` },
    ],
    { duration: 420, easing: 'cubic-bezier(0.2, 0.9, 0.25, 1)' },
  );
};

export const installLiquidTabs = (root: HTMLElement) => {
  const all = (animate: boolean) => root.querySelectorAll<HTMLElement>('.v4-tabs').forEach((g) => place(g, animate));
  all(false);
  const mo = new MutationObserver((records) => {
    const groups = new Set<HTMLElement>();
    let added = false;
    for (const r of records) {
      if (r.type === 'attributes') {
        const g = (r.target as HTMLElement).closest<HTMLElement>('.v4-tabs');
        if (g) groups.add(g);
      } else added = true;
    }
    groups.forEach((g) => place(g, true));
    if (added) requestAnimationFrame(() => all(false));
  });
  mo.observe(root, { subtree: true, childList: true, attributes: true, attributeFilter: ['aria-pressed'] });
  const ro = new ResizeObserver(() => all(false));
  ro.observe(root);
  return () => { mo.disconnect(); ro.disconnect(); };
};
