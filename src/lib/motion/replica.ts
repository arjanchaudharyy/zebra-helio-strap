export interface ReplicaRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

const SKIP_ATTRS = new Set(['id', 'style', 'href', 'tabindex', 'role', 'aria-current', 'aria-label', 'title', 'inert']);

const offsetIn = (box: DOMRect, host: { rect: DOMRect; left: number; top: number }) => ({
  left: `${box.left - host.rect.left - host.left}px`,
  top: `${box.top - host.rect.top - host.top}px`,
});

const FROZEN: Partial<CSSStyleDeclaration> = {
  position: 'absolute',
  margin: '0',
  boxSizing: 'border-box',
  minWidth: '0',
  minHeight: '0',
  maxWidth: 'none',
  maxHeight: 'none',
  transform: 'none',
  translate: 'none',
  scale: 'none',
  rotate: 'none',
  pointerEvents: 'none',
};

const decorative = (node: Element): node is HTMLElement => {
  if (!(node instanceof HTMLElement) || node.getAttribute('aria-hidden') !== 'true') return false;
  const computed = getComputedStyle(node);
  return (computed.position === 'absolute' || computed.position === 'fixed') && computed.pointerEvents === 'none';
};

const MAX_SIBLINGS = 80;
const placeLike = (parent: HTMLElement, child: HTMLElement, original: HTMLElement) => {
  const siblings = original.parentElement ? [...original.parentElement.children] : [];
  if (!siblings.includes(original) || siblings.length > MAX_SIBLINGS) {
    parent.appendChild(child);
    return;
  }
  for (const sibling of siblings) {
    if (sibling === original) parent.appendChild(child);
    else if (decorative(sibling)) parent.appendChild(sibling.cloneNode(true));
    else parent.appendChild(document.createElement('template'));
  }
};

const ancestorsOf = (card: HTMLElement): HTMLElement[] => {
  const chain: HTMLElement[] = [];
  for (let node = card.parentElement; node; node = node.parentElement) {
    if (node.classList.contains('page-host') || node.id === 'main-content' || node === document.body) break;
    chain.unshift(node);
  }
  return chain;
};

const shellOf = (node: HTMLElement): HTMLElement => {
  const shell = document.createElement(node.tagName.toLowerCase());
  for (const attr of node.attributes) {
    if (!SKIP_ATTRS.has(attr.name)) shell.setAttribute(attr.name, attr.value);
  }
  for (const name of node.style) {
    if (name.startsWith('--')) shell.style.setProperty(name, node.style.getPropertyValue(name));
  }
  return shell;
};

const copyCanvases = (from: HTMLElement, to: HTMLElement) => {
  const sources = from.querySelectorAll('canvas');
  const targets = to.querySelectorAll('canvas');
  sources.forEach((source, index) => {
    const target = targets[index];
    if (!target || !source.width || !source.height) return;
    target.width = source.width;
    target.height = source.height;
    try {
      target.getContext('2d')?.drawImage(source, 0, 0);
    } catch {
    }
  });
};

export function cardReplica(card: HTMLElement): { el: HTMLElement; rect: ReplicaRect } {
  const box = card.getBoundingClientRect();
  const style = getComputedStyle(card);

  const el = document.createElement('div');
  el.setAttribute('aria-hidden', 'true');
  el.setAttribute('inert', '');
  el.className = 'window-replica';
  Object.assign(el.style, {
    position: 'absolute',
    left: '0px',
    top: '0px',
    width: `${box.width}px`,
    height: `${box.height}px`,
    overflow: 'hidden',
    borderRadius: style.borderRadius,
    pointerEvents: 'none',
    transformOrigin: '50% 50%',
    willChange: 'transform, opacity',
  });

  let parent: HTMLElement = el;
  let host = { rect: box, left: 0, top: 0 };
  ancestorsOf(card).forEach((node, depth) => {
    const shell = shellOf(node);
    const computed = getComputedStyle(node);
    if (computed.display === 'contents') {
      shell.style.display = 'contents';
    } else {
      const rect = node.getBoundingClientRect();
      Object.assign(shell.style, FROZEN, offsetIn(rect, host), {
        width: `${rect.width}px`,
        height: `${rect.height}px`,
        overflow: computed.overflowX === 'visible' && computed.overflowY === 'visible' ? 'visible' : 'hidden',
      });
      host = { rect, left: node.clientLeft, top: node.clientTop };
    }
    if (depth === 0) parent.appendChild(shell);
    else placeLike(parent, shell, node);
    parent = shell;
  });

  const copy = card.cloneNode(true) as HTMLElement;
  copy.removeAttribute('href');
  copy.removeAttribute('id');
  copy.removeAttribute('inert');
  copy.removeAttribute('aria-hidden');
  for (const node of copy.querySelectorAll('[id]')) node.removeAttribute('id');
  Object.assign(copy.style, FROZEN, offsetIn(box, host), {
    width: `${box.width}px`,
    height: `${box.height}px`,
    opacity: '1',
    visibility: 'visible',
    backgroundImage: style.backgroundImage,
    backgroundColor: style.backgroundColor,
    boxShadow: style.boxShadow,
    borderColor: style.borderColor,
    color: style.color,
  });
  copyCanvases(card, copy);
  if (parent === el) parent.appendChild(copy);
  else placeLike(parent, copy, card);

  return { el, rect: { left: box.left, top: box.top, width: box.width, height: box.height } };
}
