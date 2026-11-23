
export interface AxisTick {
  at: number;
  label: string;
}

export function gateAxisTicks<T extends AxisTick>(ticks: readonly T[]): T[] {
  const seen = new Set<string>();
  const out: T[] = [];
  let last = -Infinity;
  for (const tick of ticks) {
    if (!(tick.at > last) || seen.has(tick.label)) continue;
    seen.add(tick.label);
    out.push(tick);
    last = tick.at;
  }
  return out;
}

export function axisTickProblems(ticks: readonly AxisTick[]): string[] {
  const problems: string[] = [];
  const seen = new Set<string>();
  ticks.forEach((tick, index) => {
    if (index > 0 && !(tick.at > ticks[index - 1].at)) problems.push(`#${index} ${tick.label} not increasing`);
    if (seen.has(tick.label)) problems.push(`#${index} ${tick.label} repeated`);
    seen.add(tick.label);
  });
  return problems;
}

const HOUR = 3_600_000;

export function sleepHourTicks(from: number, span: number, format: (ms: number) => string): AxisTick[] {
  if (!(span > 0)) return [];
  const hours = span / HOUR;
  const step = hours <= 6 ? 1 : hours <= 12 ? 2 : 3;
  const first = new Date(from);
  first.setMinutes(0, 0, 0);
  const out: AxisTick[] = [];
  for (let at = first.getTime() + HOUR; at < from + span; at += HOUR) {
    if (new Date(at).getHours() % step !== 0) continue;
    const left = ((at - from) / span) * 100;
    if (left < 6 || left > 94) continue;
    out.push({ at: left, label: format(at) });
  }
  const ends = new Set([format(from), format(from + span)]);
  return gateAxisTicks(out.filter((tick) => !ends.has(tick.label)));
}
