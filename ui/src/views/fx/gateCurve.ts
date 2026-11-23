// The Gate's static curve (crates/engine/src/dsp/gate.rs), apart from the canvas code so tests can use it.

/** The gate's static curve: output dB for a steady input at `x` dB (gate closed below the close threshold). */
export function gateCurve(p: Record<string, number>, x: number): number {
  const thr = p.thr ?? -40, close = thr - (p.hyst ?? 4), ratio = p.ratio ?? 100, floor = p.floor ?? -80;
  const fl = floor <= -79.9 ? -Infinity : floor;
  const duck = (p.mode ?? 0) >= 0.5;
  let g: number;
  if (x >= close) g = 0;
  else if (ratio >= 99.5) g = fl;
  else g = Math.max((ratio - 1) * (x - close), fl);
  if (duck) g = (fl === -Infinity ? -120 : fl) - (g === -Infinity ? -120 : g);
  return x + g;
}
