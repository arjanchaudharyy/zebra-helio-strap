
export const VIEWPORT_MARGIN = 8;

export interface TriggerRect {
  top: number;
  bottom: number;
  left: number;
  width: number;
}

export interface Viewport {
  width: number;
  height: number;
}

export interface PopoverSizing {
  maxHeight: number;
  width?: number;
  preferUp?: boolean;
  minHeight?: number;
}

export interface PopoverStyle {
  position: 'fixed';
  left: string;
  width?: string;
  maxHeight: string;
  top?: string;
  bottom?: string;
}

export const resolveDropUp = (
  trigger: TriggerRect,
  viewport: Viewport,
  sizing: PopoverSizing,
): boolean => {
  const spaceBelow = viewport.height - trigger.bottom - VIEWPORT_MARGIN;
  const spaceAbove = trigger.top - VIEWPORT_MARGIN;
  return sizing.preferUp
    ? spaceAbove > sizing.maxHeight || spaceAbove > spaceBelow
    : spaceBelow < Math.min(sizing.maxHeight, 160) && spaceAbove > spaceBelow;
};

export const anchoredLeft = (
  anchor: { left: number; right: number },
  width: number,
  viewportWidth: number,
  margin: number,
): number => {
  const max = Math.max(margin, viewportWidth - width - margin);
  if (anchor.left + width <= viewportWidth - margin) return Math.max(margin, Math.min(anchor.left, max));
  if (anchor.right - width >= margin) return Math.min(anchor.right - width, max);
  return Math.max(margin, Math.min((anchor.left + anchor.right) / 2 - width / 2, max));
};

export const popoverStyle = (
  trigger: TriggerRect,
  viewport: Viewport,
  sizing: PopoverSizing,
): PopoverStyle => {
  const dropUp = resolveDropUp(trigger, viewport, sizing);
  const spaceBelow = viewport.height - trigger.bottom - VIEWPORT_MARGIN;
  const spaceAbove = trigger.top - VIEWPORT_MARGIN;
  const minHeight = sizing.minHeight ?? 120;
  const available = Math.max(minHeight, Math.floor(dropUp ? spaceAbove : spaceBelow));

  const width = sizing.width ?? trigger.width;
  const maxLeft = Math.max(VIEWPORT_MARGIN, viewport.width - width - VIEWPORT_MARGIN);
  const left = Math.round(Math.min(Math.max(VIEWPORT_MARGIN, trigger.left), maxLeft));

  return {
    position: 'fixed',
    left: `${left}px`,
    width: `${Math.round(width)}px`,
    maxHeight: `${Math.min(sizing.maxHeight, available)}px`,
    ...(dropUp
      ? { bottom: `${Math.round(viewport.height - trigger.top + 4)}px` }
      : { top: `${Math.round(trigger.bottom + 4)}px` }),
  };
};
