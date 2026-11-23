// The browser mock's Gate and Filter (dsp::gate, dsp::filter), stepped once per meter frame: the
// gate's open / hold / closing states with its hold time, and the filter's envelope follower with
// its attack and release. Kept apart from mock.ts so tests can step them.

/** Gate state between frames: `state` as meter [2] (0 closed, 1 open, 2 hold, 3 closing). */
export interface GateSim { state: number; hold: number; gr: number }

/** One frame of the gate: `det` is the detector level (dBFS), `dt` the time since the last (s). */
export function gateStep(pr: Record<string, number>, det: number, dt: number, s: GateSim): number {
  const thr = pr.thr ?? -40, close = thr - (pr.hyst ?? 4), ratio = pr.ratio ?? 100;
  const floor = (pr.floor ?? -80) <= -79.9 ? -120 : pr.floor ?? -80, duck = (pr.mode ?? 0) >= 0.5;
  const holdS = (pr.hold ?? 40) / 1000;
  // open at the threshold; below the close threshold the hold runs out before it closes
  if (s.state === 0 || s.state === 3) {
    if (det >= thr) { s.state = 1; s.hold = holdS; }
  } else if (det >= close) {
    s.state = 1; s.hold = holdS;
  } else if (s.hold > 0) {
    s.state = 2; s.hold -= dt;
  } else {
    s.state = 3;
  }
  const open = s.state === 1 || s.state === 2;
  const shut = ratio >= 99.5 ? floor : Math.max(floor, (ratio - 1) * Math.min(0, det - close));
  const gateDb = open ? 0 : shut, target = duck ? floor - gateDb : gateDb;
  // ducking swaps them: attack is how fast the sound gets out of the way
  const att = (duck ? pr.rel ?? 120 : pr.att ?? 1) / 1000, rel = (duck ? pr.att ?? 1 : pr.rel ?? 120) / 1000;
  const prev = s.gr;
  s.gr = target + (prev - target) * Math.exp(-dt / (target > prev ? att : rel));
  if (s.state === 3 && Math.abs(s.gr - (duck ? 0 : floor)) < 0.1) s.state = 0;
  return Math.max(-120, s.gr);
}

/** One frame of the filter's envelope follower (0..1 from −40..0 dBFS), with its attack and release. */
export function filterEnvStep(pr: Record<string, number>, detDb: number, dt: number, env: number): number {
  const target = Math.min(1, Math.max(0, (detDb + 40) / 40));
  const tau = (target > env ? pr.eatt ?? 10 : pr.erel ?? 150) / 1000;
  return target + (env - target) * Math.exp(-dt / Math.max(1e-4, tau));
}
