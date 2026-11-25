// Convert to MIDI, melody: the browser mock's port of `crates/engine/src/audio_midi.rs` (the app
// always converts in the engine; this keeps the mock answering the same notes). Keep in step with it.

/** A note of the audio: source seconds, the pitch it sounds at (MIDI, fractional), its mean level. */
export interface HeardNote { start: number; end: number; pitch: number; level: number }
/** What the dialog sets (`audio_to_midi` args): seconds, steps (0 = as sung), 0..1 or null, MIDI keys or null. */
export interface MelodyOptions { shortest: number; quantize: number; velocity: number | null; low: number | null; high: number | null }
export const MELODY_DEFAULTS: MelodyOptions = { shortest: 0.06, quantize: 0, velocity: null, low: null, high: null };
/** A converted note on the song: steps from the song's start. */
export interface SongNote { step: number; len: number; p: number; v: number }

const STEPS_PER_BEAT = 4;
export const MIN_STEPS = 0.25;
const r3 = (x: number) => Math.round(x * 1000) / 1000;

export function melodyNotes(heard: readonly HeardNote[], region: [number, number], beatOf: (t: number) => number, o: MelodyOptions): SongNote[] {
  const ns = heard
    .filter(n => Number.isFinite(n.pitch) && n.end > n.start)
    .map(n => ({ ...n, start: Math.max(n.start, region[0]), end: Math.min(n.end, region[1]) }))
    .filter(n => n.end - n.start >= Math.max(o.shortest, 1e-6))
    .filter(n => { const k = Math.round(n.pitch); return (o.low == null || k >= o.low) && (o.high == null || k <= o.high); })
    .sort((a, b) => a.start - b.start);
  const loud = ns.reduce((m, n) => Math.max(m, n.level), 1e-9);
  const q = Math.max(0, o.quantize);
  const snap = (s: number) => (q > 0 ? Math.round(s / q) * q : s);
  const out: SongNote[] = [];
  for (const n of ns) {
    const a = beatOf(n.start) * STEPS_PER_BEAT, b = beatOf(n.end) * STEPS_PER_BEAT;
    const s = r3(snap(a));
    const e = r3(Math.max(snap(b), s + Math.max(q, MIN_STEPS)));
    const v = Math.round(Math.min(1, Math.max(0, o.velocity ?? 0.35 + 0.65 * Math.min(1, Math.max(0, n.level / loud)))) * 100) / 100;
    const p = Math.min(127, Math.max(0, Math.round(n.pitch)));
    const prev = out[out.length - 1];
    if (prev && prev.step + prev.len > s) {
      prev.len = r3(s - prev.step);
      if (prev.len < MIN_STEPS) out.pop();
    }
    if (out.length && out[out.length - 1].step >= s) out.pop();
    out.push({ step: s, len: Math.max(MIN_STEPS, r3(e - s)), p, v });
  }
  return out;
}
