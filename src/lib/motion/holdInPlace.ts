export const holdInPlace = (el: Element): void => {
  if (!(el instanceof HTMLElement)) return;
  const { offsetTop, offsetLeft, offsetWidth, offsetHeight } = el;
  Object.assign(el.style, {
    position: 'absolute',
    top: `${offsetTop}px`,
    left: `${offsetLeft}px`,
    width: `${offsetWidth}px`,
    height: `${offsetHeight}px`,
    margin: '0',
  });
};
