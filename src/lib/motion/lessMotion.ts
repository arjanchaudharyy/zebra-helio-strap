export const lessMotion = (): boolean => {
  if (typeof document !== 'undefined' && document.documentElement?.dataset?.effects === 'lite') return true;
  return typeof window !== 'undefined' && (window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false);
};
