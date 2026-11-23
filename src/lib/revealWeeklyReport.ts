import { lessMotion } from './motion/lessMotion';
export const revealWeeklyReport = (): void => {
  const started = performance.now();
  const tick = () => {
    const report = document.querySelector<HTMLElement>('.weekly-card');
    if (report) {
      const reduce = lessMotion();
      report.scrollIntoView({ behavior: reduce ? 'auto' : 'smooth', block: 'nearest' });
      return;
    }
    if (performance.now() - started < 1500) requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
};
