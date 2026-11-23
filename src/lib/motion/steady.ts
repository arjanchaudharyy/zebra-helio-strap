const GOOD_MS = 24;

let lastPeriod = Infinity;
export const goodFrameMs = (period: number): number => {
  const p = Math.max(4, period);
  return Math.min(GOOD_MS, Math.max(p * 1.5, p + 2));
};

export const whenFramesSteady = (maxMs = 260, minFrames = 2): Promise<void> =>
  new Promise((resolve) => {
    const started = performance.now();
    let last = 0;
    let frames = 0;
    let good = 0;
    let shortest = Infinity;
    const finish = () => {
      if (Number.isFinite(shortest)) lastPeriod = shortest;
      resolve();
    };
    const tick = (now: number) => {
      frames += 1;
      if (last) {
        const gap = now - last;
        shortest = Math.min(shortest, gap);
        good = gap < goodFrameMs(Math.min(lastPeriod, shortest)) ? good + 1 : 0;
      }
      last = now;
      if ((frames >= minFrames && good >= 2) || now - started > maxMs) {
        finish();
        return;
      }
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
