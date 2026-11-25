// The Master's tone curve and listen constants (crates/engine/src/dsp/master.rs, mastering.rs),
// shared by its window and the in-browser mock.
import { rbj, SR } from './eq';

/** dsp::master::TARGETS, in the `target` parameter's order (integrated LUFS). */
export const MASTER_TARGETS = [-16, -14, -12, -10, -8];
/** mastering::THIRDS: the third-octave centres a listen's balance is reported at. */
export const THIRDS = [20, 25, 31.5, 40, 50, 63, 80, 100, 125, 160, 200, 250, 315, 400, 500, 630, 800, 1000,
  1250, 1600, 2000, 2500, 3150, 4000, 5000, 6300, 8000, 10000, 12500, 16000, 20000];
/** The five tone bands: param id, RBJ kind, Hz, Q (dsp::master::TONE_BANDS). */
export const TONE_BANDS = [['sub', 'ls', 70, 0.7], ['low', 'peak', 200, 0.8], ['mid', 'peak', 800, 0.7], ['pres', 'peak', 3500, 0.8], ['air', 'hs', 10000, 0.7]] as const;

function respDb(c: [number, number, number, number, number], f: number, sr: number): number {
  const [b0, b1, b2, a1, a2] = c, w = 2 * Math.PI * f / sr;
  const c1 = Math.cos(w), s1 = Math.sin(w), c2 = Math.cos(2 * w), s2 = Math.sin(2 * w);
  const nr = b0 + b1 * c1 + b2 * c2, ni = -(b1 * s1 + b2 * s2), dr = 1 + a1 * c1 + a2 * c2, di = -(a1 * s1 + a2 * s2);
  return 10 * Math.log10(Math.max(1e-30, (nr * nr + ni * ni) / (dr * dr + di * di)));
}

/** The tone EQ's response at `f` Hz for a Master's params (gains × Tone). */
export function toneDb(params: Record<string, number>, f: number, sr = SR): number {
  const amt = Math.min(1, Math.max(0, (params.tone ?? 100) / 100));
  let db = 0;
  for (const [id, kind, fc, q] of TONE_BANDS) {
    const g = Math.max(-6, Math.min(6, params[id] ?? 0)) * amt;
    if (Math.abs(g) > 1e-4) db += respDb(rbj(kind, fc, q, g, sr), f, sr);
  }
  return db;
}

/** mastering::HEAVY_CUT_DB: peak reduction past which the window warns. */
export const HEAVY_CUT_DB = 8;

/** "3:07" for seconds. */
export const mmss = (s: number) => `${Math.floor(s / 60)}:${String(Math.round(s % 60)).padStart(2, '0')}`;
