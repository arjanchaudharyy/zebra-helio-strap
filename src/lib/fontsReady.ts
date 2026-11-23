let shared: Promise<unknown> | null = null;

export const fontsReady = (): Promise<unknown> => {
  if (!shared) shared = typeof document !== 'undefined' && document.fonts ? document.fonts.ready : Promise.resolve();
  return shared;
};
