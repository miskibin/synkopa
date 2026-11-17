// The TypeScript mirror of `crates/engine/src/transform.rs`: the MockBackend applies
// the same seeded, deterministic note transforms as the engine, bit for bit (same
// splitmix64, same rounding), so browser demos and engine-driven sessions agree.
import type { Note, TransformSelect, TransformVerb } from '../api/types';
import { scaleMask } from './pattern';

/** splitmix64 — the engine's PRNG; keep identical to the Rust implementation. */
export class SplitMix64 {
  private s: bigint;
  private static readonly GOLDEN = 0x9e3779b97f4a7c15n;
  private static readonly M1 = 0xbf58476d1ce4e5b9n;
  private static readonly M2 = 0x94d049bb133111ebn;

  constructor(seed = 0) { this.s = BigInt(Math.max(0, Math.floor(seed))) + SplitMix64.GOLDEN; }
  next(): bigint {
    this.s = (this.s + SplitMix64.GOLDEN) & 0xffff_ffff_ffff_ffffn;
    let z = this.s;
    z = ((z ^ (z >> 30n)) * SplitMix64.M1) & 0xffff_ffff_ffff_ffffn;
    z = ((z ^ (z >> 27n)) * SplitMix64.M2) & 0xffff_ffff_ffff_ffffn;
    return z ^ (z >> 31n);
  }
  /** Uniform −1..1 (the engine computes this in f32: round where it rounds). */
  pm1(): number { return Math.fround(Math.fround(Number(this.next() >> 11n) / 2 ** 53) * 2 - 1); }
  unit(): number { return Math.fround(Number(this.next() >> 11n) / 2 ** 53); }
  below(n: number): number { return Number(this.next() % BigInt(n)); }
}

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

// ---------------------------------------------------------------------------
// chord tools — the mirror of transform.rs's chord tables and helpers
// ---------------------------------------------------------------------------

/** The stampable chord types in picker order: `[id, semitone intervals from the root]`.
 * The engine pins the same table (`CHORD_TYPES` in transform.rs) and the golden tests
 * assert the same voicings on both sides. */
export const CHORD_TYPES: [string, number[]][] = [
  ['maj', [0, 4, 7]],
  ['min', [0, 3, 7]],
  ['dim', [0, 3, 6]],
  ['aug', [0, 4, 8]],
  ['sus2', [0, 2, 7]],
  ['sus4', [0, 5, 7]],
  ['7', [0, 4, 7, 10]],
  ['maj7', [0, 4, 7, 11]],
  ['min7', [0, 3, 7, 10]],
  ['9', [0, 4, 7, 10, 14]],
  ['add9', [0, 4, 7, 14]],
  ['maj9', [0, 4, 7, 11, 14]],
  ['min9', [0, 3, 7, 10, 14]],
  ['6', [0, 4, 7, 9]],
  ['m6', [0, 3, 7, 9]],
  ['dim7', [0, 3, 6, 9]],
  ['m7b5', [0, 3, 6, 10]],
  ['5', [0, 7]],
];

const CHORD_SUFFIX: Record<string, string> = {
  maj: '', min: 'm', dim: '°', aug: '+', sus2: 'sus2', sus4: 'sus4', 7: '7', maj7: 'maj7',
  min7: 'm7', 9: '9', add9: 'add9', maj9: 'maj9', min9: 'm9', 6: '6', m6: 'm6', dim7: '°7',
  m7b5: 'm7♭5', 5: '5',
};

/** Harmony types the `chord` verb can spell; the stamp picker's exotic types stay stamp-only. */
export const HARMONY_TYPES = ['diatonic', 'maj', 'min', '7', 'maj7', 'min7'];

/** Progression templates: one diatonic 7th chord per bar, as scale-degree indices. */
export const PROGRESSION_TEMPLATES: [string, number[]][] = [
  ['pop', [0, 4, 5, 3]],               // I V vi IV
  ['pop2', [5, 3, 0, 4]],              // vi IV I V
  ['minloop', [0, 5, 2, 6]],           // i VI III VII
  ['trap', [0, 3, 5, 4]],              // i iv VI v
  ['iivI', [1, 4, 0]],                 // ii V I
  ['doowop', [0, 5, 3, 4]],            // I vi IV V
  ['andalusian', [0, 6, 5, 4]],        // i VII VI v
  ['blues', [0, 3, 4]],                // i iv v
  ['canon', [0, 4, 5, 2, 3, 0, 3, 4]], // 8-bar canon cycle
];

const NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

/** Absolute pitches of a chord rooted at `root` (MIDI), rotated `inversion` times (each
 * rotation lifts the lowest voice an octave, clamped to the last inversion), ascending.
 * `null` for an unknown type or a root pushing the top voice past MIDI 127. */
export function chordPitches(root: number, typeId: string, inversion = 0): number[] | null {
  const t = CHORD_TYPES.find(([id]) => id === typeId);
  if (!t || !Number.isInteger(root) || root < 0) return null;
  const p = t[1].map(v => root + v);
  for (let k = 0; k < Math.min(inversion, t[1].length - 1); k++) {
    let lo = 0;
    p.forEach((v, i) => { if (v < p[lo]) lo = i; });
    p[lo] += 12;
  }
  // the limit is checked after rotating: lifting a voice can push the top past 127
  if (p.some(v => v > 127)) return null;
  return p.sort((a, b) => a - b);
}

/** The chord a pitch set spells, bass-first: the lowest note's pitch class is tried as
 * the root first, so a root-position C6 is "C6", never "Am7". Unknown sets → "X?". */
export function chordName(pitches: number[]): string {
  if (!pitches.length) return '?';
  const bass = ((Math.min(...pitches) % 12) + 12) % 12;
  const pcs = [...new Set(pitches.map(p => ((p % 12) + 12) % 12))].sort((a, b) => a - b);
  for (const pc of [bass, ...pcs.filter(pc => pc !== bass)]) {
    const rel = pcs.map(p => (p - pc + 12) % 12).sort((a, b) => a - b).join(',');
    for (const [id, iv] of CHORD_TYPES) {
      const sig = [...new Set(iv.map(v => v % 12))].sort((a, b) => a - b).join(',');
      if (rel === sig) return NOTE_NAMES[pc] + CHORD_SUFFIX[id];
    }
  }
  return NOTE_NAMES[bass] + '?';
}

/** The scale's semitone offsets from the key, ascending (the set bits of the mask). */
function scaleOffsets(scale: number): number[] {
  const mask = scaleMask(scale);
  const out: number[] = [];
  for (let k = 0; k < 12; k++) if (mask & (1 << k)) out.push(k);
  return out;
}

/** The diatonic stack on scale degree `deg` — degrees deg, deg+2, deg+4 (+deg+6 for a
 * seventh) — as semitones above the key, octave-wrapped. */
function diatonicStack(scale: number, deg: number, seventh: boolean): number[] {
  const offs = scaleOffsets(scale);
  const n = offs.length;
  return (seventh ? [0, 2, 4, 6] : [0, 2, 4]).map(i => {
    const k = deg + i;
    return offs[k % n] + 12 * Math.floor(k / n);
  });
}

/** Expand a progression mode into per-bar scale degrees: a named template cycles, or
 * `chance` walks ±2 degrees from a seeded random start. `null` for an unknown id. */
export function progressionDegrees(mode: string, bars: number, seed: number, scaleDegrees: number): number[] | null {
  const n = scaleDegrees;
  if (mode === 'chance') {
    const rng = new SplitMix64(seed);
    let d = rng.below(n);
    const out: number[] = [];
    for (let b = 0; b < bars; b++) {
      out.push(d);
      d = (d + rng.below(5) + n - 2) % Math.max(1, n);
    }
    return out;
  }
  const t = PROGRESSION_TEMPLATES.find(([id]) => id === mode);
  return t ? Array.from({ length: bars }, (_, b) => t[1][b % t[1].length]) : null;
}

/** The engine's `transform::snap_midi` (`pitch::snap` within MIDI keys): nearest
 * in-scale key in 0..=127, ties keep the lower note. */
export function snapMidi(m: number, key: number, mask: number): number {
  const base = Math.round(m);
  let best: number | null = null, bd = Infinity;
  for (let d = -11; d <= 11; d++) {
    const n = base + d;
    if (n < 0 || n > 127) continue;
    const pc = ((n - key) % 12 + 12) % 12;
    if (mask & (1 << pc)) {
      const dist = Math.abs(n - m);
      if (dist < bd) { bd = dist; best = n; }
    }
  }
  return Math.min(127, Math.max(0, best ?? base));
}

function euclideanOnsets(k: number, n: number, rotate: number): boolean[] {
  k = Math.max(1, k); n = Math.max(1, n);
  return Array.from({ length: n }, (_, i) => {
    const slot = ((i + rotate) % n + n) % n;
    return (slot * k) % n < k;
  });
}

/** One `transformNotes` op against a pattern's notes; mutates `notes` in place. Throws on invalid params. */
export function applyTransform(notes: Note[], verb: TransformVerb, seed: number, select: TransformSelect | null, roots: { id: number; root: number }[], key: number | null, scale: number, lenSteps: number): void {
  const rng = new SplitMix64(seed);
  const chanOk = (id: number) => roots.some(r => r.id === id);
  if ((verb.kind === 'euclidFill' || verb.kind === 'melody') && !chanOk(verb.ch)) throw new Error(`no channel with id ${verb.ch}`);
  if (verb.kind === 'euclidDrums' && verb.voices.some(v => !chanOk(v.ch))) throw new Error('no channel with that id');

  // generators replace every note of their target channel(s) and keep the other
  // channels, so a selection does not scope them (it would leave unselected notes of the
  // target channel under the new ones); it is still validated, like the engine does
  const generative = verb.kind === 'euclidFill' || verb.kind === 'euclidDrums' || verb.kind === 'melody' || verb.kind === 'progression';
  // the notes the verb sees — the engine builds the same index list
  const selIdx = !select
    ? notes.map((_, i) => i)
    : select.kind === 'indices'
      ? (select.indices.some(i => i >= notes.length)
        ? (() => { throw new Error('select: a note index is out of range'); })()
        : [...select.indices])
      : notes.map((n, i) => (n.step >= Math.min(select.from, select.to) && n.step < Math.max(select.from, select.to) && (!select.ch || n.ch === select.ch) ? i : -1))
          .filter(i => i >= 0);
  const idx = generative ? notes.map((_, i) => i) : selIdx;
  const target: Note[] = idx.map(i => structuredClone(notes[i]));

  switch (verb.kind) {
    case 'chord': {
      const harmony = verb.harmony ?? 'diatonic';
      if (!HARMONY_TYPES.includes(harmony)) {
        throw new Error(`chord harmony must be one of ${HARMONY_TYPES.join(', ')} (the other chord types are stamp-only)`);
      }
      if (harmony === 'diatonic' && (key == null || (scale ?? 1) === 0)) {
        throw new Error('chord diatonic needs a project key and scale (`wave key A minor`)');
      }
      const groups = new Map<string, number[]>();
      target.forEach((n, i) => {
        const k = `${n.ch}:${n.step}`;
        const g = groups.get(k);
        if (g) g.push(i); else groups.set(k, [i]);
      });
      // process groups in (channel, step) order: appended tones land deterministically
      const order = [...groups.keys()].sort((a, b) => {
        const [ca, sa] = a.split(':').map(Number);
        const [cb, sb] = b.split(':').map(Number);
        return ca - cb || sa - sb;
      });
      for (const k of order) {
        const g = groups.get(k)!;
        if (g.length !== 1) continue; // a step that already has a chord is not a root
        const i = g[0];
        const n = target[i];
        const root = n.p ?? roots.find(r => r.id === n.ch)?.root;
        if (root == null) continue;
        let ivs: number[];
        if (harmony === 'diatonic') {
          const pc = root % 12;
          const deg = scaleOffsets(scale ?? 1).findIndex(o => ((key ?? 0) + o) % 12 === pc);
          if (deg < 0) continue; // out of scale: leave it alone
          const stack = diatonicStack(scale ?? 1, deg, false);
          ivs = stack.slice(1).map(s => s - stack[0]);
        } else {
          const t = CHORD_TYPES.find(([id]) => id === harmony);
          if (!t) continue;
          ivs = t[1].slice(1);
        }
        const vel = Math.fround(Math.min(1, Math.max(0.05, Math.fround((n.v ?? 1) * 0.9))));
        for (const v of ivs) {
          const p = root + v;
          // a tone already sounding at this step is not doubled — checked against the
          // full pattern (unselected notes, other channels, implicit channel roots)
          // and against tones added earlier in this run
          const sounding = (x: Note) => x.step === n.step && (x.p ?? roots.find(r => r.id === x.ch)?.root) === p;
          if (!target.some(sounding) && !notes.some(sounding)) {
            target.push({ ch: n.ch, step: n.step, len: n.len ?? 1, p, v: vel, trig: false });
          }
        }
      }
      break;
    }
    case 'progression': {
      const ch = verb.ch;
      if (!chanOk(ch)) throw new Error(`no channel with id ${ch}`);
      if (key == null || (scale ?? 1) === 0) throw new Error('progression needs a project key and scale (`wave key A minor`)');
      const n = scaleOffsets(scale ?? 1).length;
      const degrees = verb.degrees ?? [];
      if (degrees.length) {
        if (degrees.length > 32) throw new Error('progression degrees: at most 32 entries');
        const bad = degrees.find(d => !Number.isInteger(d) || d < 0 || d >= n);
        if (bad != null) throw new Error(`progression degrees must be 0..${n - 1} in this scale (got ${bad})`);
      } else {
        const bars = verb.bars ?? 4;
        if (bars < 1 || bars > 32) throw new Error('progression bars must be 1..=32');
        const template = verb.template ?? 'pop';
        if (template !== 'chance' && !PROGRESSION_TEMPLATES.some(([id]) => id === template)) {
          throw new Error(`unknown progression template \`${template}\` (${PROGRESSION_TEMPLATES.map(([id]) => id).join(', ')}, chance)`);
        }
      }
      const degs = degrees.length
        ? degrees
        : progressionDegrees(verb.template ?? 'pop', Math.max(1, verb.bars ?? 4), seed, n) ?? [];
      // the generator owns its channel: like euclidFill, its previous notes make way
      for (let i = target.length - 1; i >= 0; i--) if (target[i].ch === ch) target.splice(i, 1);
      const chords = degs.map(d => {
        const stack = diatonicStack(scale ?? 1, d, true);
        const base = 48 + (((key ?? 0) + stack[0]) % 12);
        return stack.map(s => base + (s - stack[0]));
      });
      if (verb.voiceLead ?? true) {
        for (let i = 1; i < chords.length; i++) {
          const prev = [...chords[i - 1]].sort((a, b) => a - b);
          let best: { cost: number; cand: number[] } | null = null;
          for (let r = 0; r < chords[i].length; r++) {
            const cand = [...chords[i]];
            for (let k = 0; k < r; k++) cand[k] += 12;
            const sorted = [...cand].sort((a, b) => a - b);
            let cost = 0;
            sorted.forEach((v, j) => { cost += Math.abs(v - prev[Math.min(j, prev.length - 1)]); });
            if (!best || cost < best.cost) best = { cost, cand };
          }
          if (best) chords[i] = best.cand;
        }
      }
      chords.forEach((chd, bar) => {
        [...chd].sort((a, b) => a - b).forEach(p => target.push({ ch, step: bar * 16, len: 16, p, v: 0.75, trig: false }));
      });
      break;
    }
    case 'humanize': {
      const maxJitter = Math.fround(0.34 * clamp01(verb.timing ?? 0.5));
      const vel = clamp01(verb.velocity ?? 0.5);
      for (const n of target) {
        n.step = Math.max(0, Math.fround(n.step + Math.fround(rng.pm1() * maxJitter)));
        const nudge = Math.fround(1 + Math.fround(Math.fround(rng.pm1() * 0.5) * vel));
        n.v = Math.min(1, Math.max(0, Math.fround(Math.fround((n.v ?? 1) * nudge))));
      }
      break;
    }
    case 'quantize': {
      const grid = Math.fround(Math.max(0.25, verb.grid ?? 1));
      const amount = Math.fround(clamp01(verb.amount ?? 1));
      const ks = verb.scaleAware && key != null ? { key, mask: scaleMask(scale ?? 1) } : null;
      for (const n of target) {
        const tgt = Math.fround(Math.round(Math.fround(n.step / grid)) * grid);
        n.step = Math.fround(n.step + Math.fround((tgt - n.step) * amount));
        n.v = n.v ?? 1;
        if (ks) {
          const root = roots.find(r => r.id === n.ch)?.root ?? 60;
          n.p = snapMidi(n.p ?? root, ks.key, ks.mask);
        }
      }
      break;
    }
    case 'euclidFill': {
      for (let i = target.length - 1; i >= 0; i--) if (target[i].ch === verb.ch) target.splice(i, 1);
      const onsets = euclideanOnsets(verb.k, verb.n, verb.rotate ?? 0);
      const slot = Math.fround(lenSteps / Math.max(1, verb.n));
      let hits = 0;
      onsets.forEach((on, i) => {
        if (on) {
          // every fourth emitted onset accents to full; like the engine's serde, `v` is
          // written only when it is not exactly 1 (so vel 1 = no `v` at all)
          const vel = verb.vel ?? 0.7;
          target.push({ ch: verb.ch, step: Math.fround(i * slot), len: Math.fround(Math.min(1, Math.max(0.25, slot))), trig: true, ...(hits++ % 4 === 0 || vel === 1 ? {} : { v: vel }) });
        }
      });
      break;
    }
    case 'strum': {
      // group by (channel, step) wherever the notes sit in the array
      const groups = new Map<string, number[]>();
      target.forEach((n, i) => {
        const k = `${n.ch}:${n.step}`;
        const g = groups.get(k);
        if (g) g.push(i); else groups.set(k, [i]);
      });
      const width = Math.fround(verb.width ?? 0.5);
      const ramp = Math.fround(verb.ramp ?? 0);
      for (const g of groups.values()) {
        if (g.length < 2) continue;
        g.sort((a, b) => (target[a].p ?? 60) - (target[b].p ?? 60));
        g.forEach((i, k) => {
          const n = target[i];
          n.step = Math.fround(n.step + Math.fround(k * width));
          n.v = Math.min(1, Math.max(0, Math.fround((n.v ?? 1) + Math.fround(k * ramp))));
        });
      }
      break;
    }
    case 'euclidDrums': {
      const bars = Math.max(1, verb.bars ?? 2);
      // the seed shades each voice's velocity ±15 % (its own stream, like the engine)
      const shade = new SplitMix64(seed);
      for (const v of verb.voices) {
        for (let i = target.length - 1; i >= 0; i--) if (target[i].ch === v.ch) target.splice(i, 1);
        const vel = Math.min(1, Math.max(0.1, Math.fround(Math.fround(v.vel ?? 0.7) * Math.fround(1 + Math.fround(shade.pm1() * Math.fround(0.15))))));
        const onsets = euclideanOnsets(v.k, v.n, v.rotate ?? 0);
        const slot = Math.fround((bars * 16) / Math.max(1, v.n));
        let hits = 0;
        onsets.forEach((on, i) => {
          if (on) {
            target.push({ ch: v.ch, step: Math.fround(i * slot), len: Math.fround(Math.min(1, Math.max(0.25, slot))), trig: true, p: undefined, v: hits++ % 4 === 0 ? 1 : vel });
          }
        });
      }
      break;
    }
    case 'melody': {
      // a melody replaces its channel, like euclidFill: a second run does not stack
      for (let i = target.length - 1; i >= 0; i--) if (target[i].ch === verb.ch) target.splice(i, 1);
      const bars = Math.max(1, verb.bars ?? 2);
      const rep = Math.max(1, verb.repeat ?? 1);
      const density = Math.min(1, Math.max(0.05, verb.density ?? 0.5));
      const mask = scaleMask(scale ?? 1);
      const degrees = Array.from({ length: 12 }, (_, pc) => pc).filter(pc => mask & (1 << pc));
      const n = degrees.length;
      let deg = 0;
      const seq: { d: number; len: number }[] = [];
      for (let bar = 0; bar < bars; bar++) {
        // the first `repeat` bars invent the motif; later bars replay their cycle's
        // rhythm with the walk nudged ±1 degree
        const build = bar < rep;
        for (let slot = 0; slot < 16; slot++) {
          let d: number; let len: number;
          if (build) {
            len = rng.unit() < Math.fround(0.3) ? 2 : 1;
            if (rng.unit() < density) {
              deg = Math.min(n - 1, Math.max(0, deg + [0, -1, 1, 2, -2][rng.below(5)]));
              seq.push({ d: deg, len });
              d = deg;
            } else {
              seq.push({ d: -1, len });
              d = -1;
            }
          } else {
            const src = seq[(bar % rep) * 16 + slot];
            if (src.d < 0) { d = -1; len = src.len; }
            else { deg = Math.min(n - 1, Math.max(0, deg + [0, -1, 1][rng.below(3)])); d = deg; len = src.len; }
          }
          if (d < 0) continue;
          const midi = 48 + (key ?? 0) + degrees[d % n];
          target.push({ ch: verb.ch, step: bar * 16 + slot, len, p: midi, v: 0.8, trig: false });
        }
      }
      break;
    }
  }

  if (!select || generative) {
    notes.splice(0, notes.length, ...target);
  } else if (verb.kind === 'chord') {
    // chord grew its selection (roots stay, tones appended): everything unselected
    // stays, the roots keep their slots and the appended tones follow the target
    const selected = new Set(idx);
    const merged = notes.filter((_, i) => !selected.has(i));
    merged.push(...target);
    notes.splice(0, notes.length, ...merged);
  } else {
    idx.forEach((ni, k) => { notes[ni] = target[k]; });
  }
}
