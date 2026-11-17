import { describe, expect, it } from 'vitest';
import type { Note } from '../api/types';
import { SCALES, scaleMask } from './pattern';
import { applyTransform, CHORD_TYPES, chordName, chordPitches, progressionDegrees, PROGRESSION_TEMPLATES, snapMidi, SplitMix64 } from './transforms';

const note = (ch: number, step: number, p = 60, v = 0.8): Note => ({ ch, step, len: 1, p, v });
const notes = () => [note(1, 0, 61), note(1, 4.3, 64), note(1, 8, 60)];

describe('scale ids (engine id space, pitch::scale_mask)', () => {
  it('list engine ids, not list positions', () => {
    expect(SCALES.map(s => s[1])).toEqual([1, 2, 3, 4, 0]);
  });

  it('masks match pitch.rs', () => {
    expect(scaleMask(0)).toBe(0b111111111111);
    expect(scaleMask(1)).toBe(0b101010110101);
    expect(scaleMask(2)).toBe(0b010110101101);
    expect(scaleMask(3)).toBe(0b001010010101);
    expect(scaleMask(4)).toBe(0b010010101001);
  });

  it('A natural minor = id 2 (what the roll highlight and scale-aware transforms read)', () => {
    // bits = semitones above the key: A, B, C, D, E, F, G
    expect([0, 2, 3, 5, 7, 8, 10].every(pc => scaleMask(2) & (1 << pc))).toBe(true);
    expect(scaleMask(2) & (1 << 1)).toBeFalsy();
    expect(scaleMask(2) & (1 << 6)).toBeFalsy();
  });
});

describe('transformNotes (TS mirror)', () => {
  it('humanize is deterministic in the seed', () => {
    const a = notes(), b = notes();
    const roots = [{ id: 1, root: 60 }];
    applyTransform(a, { kind: 'humanize', timing: 0.5, velocity: 0.4 }, 41, null, roots, null, 1, 16);
    applyTransform(b, { kind: 'humanize', timing: 0.5, velocity: 0.4 }, 41, null, roots, null, 1, 16);
    expect(a).toEqual(b);
  });

  it('quantize with a project key snaps into the scale', () => {
    const a = notes();
    applyTransform(a, { kind: 'quantize', grid: 1, amount: 1, scaleAware: true }, 0, null, [{ id: 1, root: 60 }], 0, 1, 16);
    expect(a.map(n => n.p)).toEqual([60, 64, 60]); // C#→C (tie), E already in scale, C in scale
  });

  it('euclidFill replaces the channel with k notes and keeps the others', () => {
    const a = [...notes(), note(2, 2, 70)];
    applyTransform(a, { kind: 'euclidFill', ch: 1, k: 5, n: 16 }, 0, null, [{ id: 1, root: 60 }, { id: 2, root: 60 }], null, 1, 16);
    expect(a.filter(n => n.ch === 1)).toHaveLength(5);
    expect(a.some(n => n.ch === 2 && n.step === 2)).toBe(true);
  });

  it('splitmix64 matches the engine by construction (stability smoke)', () => {
    const r = new SplitMix64(7);
    const seq = [r.unit(), r.unit(), r.unit()];
    const r2 = new SplitMix64(7);
    expect([r2.unit(), r2.unit(), r2.unit()]).toEqual(seq);
    expect(seq).not.toEqual([0, 0, 0]);
  });

  it('a generator replaces its whole channel whatever the selection', () => {
    const a = [note(1, 0, 60, 1), note(2, 2, 70, 1), note(1, 4, 62, 1)];
    // only the first ch-1 note is selected: the unselected one must not stay under the onsets
    applyTransform(a, { kind: 'euclidFill', ch: 1, k: 3, n: 8, rotate: 0, vel: 0.7 }, 0, { kind: 'indices', indices: [0] }, [{ id: 1, root: 60 }, { id: 2, root: 60 }], null, 1, 16);
    expect(a).toHaveLength(4);
    expect(a.filter(n => n.ch === 2)).toHaveLength(1);
    expect(a.filter(n => n.ch === 1).every(n => n.p === undefined)).toBe(true);
    // a melody likewise: selected or not, the result equals the unscoped run
    const roots = [{ id: 1, root: 60 }, { id: 2, root: 60 }];
    const m1 = notes(), m2 = notes();
    applyTransform(m1, { kind: 'melody', ch: 1, bars: 1, density: 0.5, repeat: 1 }, 3, { kind: 'indices', indices: [1] }, roots, 0, 1, 16);
    applyTransform(m2, { kind: 'melody', ch: 1, bars: 1, density: 0.5, repeat: 1 }, 3, null, roots, 0, 1, 16);
    expect(m1).toEqual(m2);
    // a bad index still fails
    expect(() => applyTransform(notes(), { kind: 'euclidFill', ch: 1, k: 3, n: 8 }, 0, { kind: 'indices', indices: [9] }, roots, null, 1, 16)).toThrow(/out of range/);
  });

  it('applying a melody twice equals applying it once', () => {
    const roots = [{ id: 1, root: 60 }, { id: 2, root: 60 }];
    const once = [...notes(), note(2, 2, 70)], twice = [...notes(), note(2, 2, 70)];
    const verb = { kind: 'melody', ch: 1, bars: 1, density: 0.5, repeat: 1 } as const;
    applyTransform(once, verb, 2, null, roots, 0, 1, 16);
    applyTransform(twice, verb, 1, null, roots, 0, 1, 16);
    applyTransform(twice, verb, 2, null, roots, 0, 1, 16);
    expect(twice).toEqual(once);
    expect(once.some(n => n.ch === 2 && n.step === 2)).toBe(true);
  });

  it('amount 0 keeps velocities below 0.05 (humanize, strum)', () => {
    const roots = [{ id: 1, root: 60 }];
    const h = [note(1, 4, 60, Math.fround(0.02))];
    applyTransform(h, { kind: 'humanize', timing: 0, velocity: 0 }, 9, null, roots, null, 1, 16);
    expect(h[0].v).toBe(Math.fround(0.02));
    const s = [note(1, 4, 60, Math.fround(0.02)), note(1, 4, 64, Math.fround(0.02))];
    applyTransform(s, { kind: 'strum', width: 0.25, ramp: 0 }, 0, null, roots, null, 1, 16);
    expect(s.every(n => n.v === Math.fround(0.02))).toBe(true);
  });

  it('scale-aware quantize stays in MIDI range at the edges', () => {
    const roots = [{ id: 1, root: 60 }];
    const top = [note(1, 0, 127)];
    applyTransform(top, { kind: 'quantize', grid: 1, amount: 1, scaleAware: true }, 0, null, roots, 1, 3, 16);
    expect(top[0].p).toBe(125); // C# major pentatonic: not 128
    const bottom = [note(1, 0, 0)];
    applyTransform(bottom, { kind: 'quantize', grid: 1, amount: 1, scaleAware: true }, 0, null, roots, 11, 3, 16);
    expect(bottom[0].p).toBe(1); // B major pentatonic: C# above, not the B below 0
    expect(snapMidi(61, 0, scaleMask(1))).toBe(60); // ties still keep the lower note
  });

  it('a range select scoped to a channel touches only that channel', () => {
    const a = [note(1, 0, 60, 1), note(2, 2, 70, 1), note(1, 8, 62, 1)];
    applyTransform(a, { kind: 'humanize', timing: 1, velocity: 0 }, 41, { kind: 'range', from: 0, to: 1e9, ch: 1 }, [{ id: 1, root: 60 }, { id: 2, root: 60 }], null, 1, 16);
    // the ch-2 note is untouched; the in-scope ch-1 note moved off step 8
    expect(a[1].step).toBe(2);
    expect(a[1].v).toBe(1);
    expect(a[2].step).not.toBe(8);
  });

  describe('golden: engine parity (values from the engine golden_fixed_seed_values test)', () => {
    const f = Math.fround;
    const noteF = (ch: number, step: number, p: number, v: number): Note => ({ ch, step, len: 1, p, v: f(v) });
    const roots = [{ id: 1, root: 60 }, { id: 3, root: 57 }];

    it('humanize', () => {
      const h = [noteF(1, 4, 60, 0.8)];
      applyTransform(h, { kind: 'humanize', timing: 0.5, velocity: 0.4 }, 41, null, roots, null, 1, 16);
      expect(h[0].step).toBe(f(4.033452));
      expect(h[0].v).toBe(f(0.67452526));
    });

    it('strum', () => {
      const s = [noteF(1, 4, 60, 0.8), noteF(1, 4, 64, 0.8), noteF(1, 4, 57, 0.8)];
      applyTransform(s, { kind: 'strum', width: 0.25, ramp: 0.05 }, 0, null, roots, null, 1, 16);
      const by = (p: number) => s.find(n => n.p === p)!;
      expect(by(57).step).toBe(4);
      expect(by(60).step).toBe(f(4.25));
      expect(by(64).step).toBe(f(4.5));
      expect(by(64).v).toBe(f(0.90000004));
    });

    it('euclidFill (accent on every fourth emitted onset)', () => {
      const e = [note(1, 2, 60, 0.9)];
      applyTransform(e, { kind: 'euclidFill', ch: 1, k: 5, n: 16, rotate: 0, vel: 0.7 }, 0, null, roots, null, 1, 16);
      expect(e.map(n => n.step)).toEqual([0, 4, 7, 10, 13]);
      expect(e.map(n => n.v ?? 1)).toEqual([1, 0.7, 0.7, 0.7, 1]); // accents write no `v` (the engine omits v == 1)
    });

    it('euclidDrums (seeded velocity shading, from golden_euclid_drums_seeded_velocity)', () => {
      const d: Note[] = [note(1, 0, 61)];
      applyTransform(d, { kind: 'euclidDrums', voices: [{ ch: 1, k: 4, n: 16, rotate: 0, vel: 0.7 }, { ch: 2, k: 3, n: 8, rotate: 1, vel: 0.5 }], bars: 1 }, 42, null, [{ id: 1, root: 60 }, { id: 2, root: 60 }], null, 1, 16);
      const of = (ch: number) => d.filter(n => n.ch === ch).map(n => [n.step, n.v]);
      expect(of(1)).toEqual([[0, 1], [4, f(0.62858117)], [8, f(0.62858117)], [12, f(0.62858117)]]);
      expect(of(2)).toEqual([[4, 1], [10, f(0.46679017)], [14, f(0.46679017)]]);
    });

    it('melody (A minor, seed 7)', () => {
      const m: Note[] = [];
      applyTransform(m, { kind: 'melody', ch: 3, bars: 2, density: 0.55, repeat: 1 }, 7, null, [{ id: 3, root: 57 }], 9, 2, 32);
      expect(m.map(n => n.p)).toEqual([57, 57, 57, 59, 59, 59, 62, 65, 64, 62, 64, 64, 62, 62, 62, 62]);
      expect(m.map(n => n.step)).toEqual([1, 2, 3, 5, 8, 10, 12, 15, 17, 18, 19, 21, 24, 26, 28, 31]);
    });
  });

  describe('chord tools (golden: engine parity with transform.rs / golden_chord_tool_values)', () => {
    const roots = [{ id: 1, root: 60 }];
    const AMINOR = { key: 9, scale: 2 };

    it('the 18-type stamp table voicings match the engine at C4', () => {
      const want: Record<string, string> = {
        maj: '60,64,67', min: '60,63,67', dim: '60,63,66', aug: '60,64,68', sus2: '60,62,67',
        sus4: '60,65,67', 7: '60,64,67,70', maj7: '60,64,67,71', min7: '60,63,67,70',
        9: '60,64,67,70,74', add9: '60,64,67,74', maj9: '60,64,67,71,74', min9: '60,63,67,70,74',
        6: '60,64,67,69', m6: '60,63,67,69', dim7: '60,63,66,69', m7b5: '60,63,66,70', 5: '60,67',
      };
      expect(CHORD_TYPES).toHaveLength(18);
      for (const [id, iv] of CHORD_TYPES) {
        expect(chordPitches(60, id)!.join(',')).toBe(want[id]);
        void iv;
      }
      expect(chordPitches(121, 'maj')).toBeNull(); // top voice would pass MIDI 127
    });

    it('inversions lift the lowest voice and clamp (min7 at A3)', () => {
      expect(chordPitches(57, 'min7', 0)).toEqual([57, 60, 64, 67]);
      expect(chordPitches(57, 'min7', 1)).toEqual([60, 64, 67, 69]);
      expect(chordPitches(57, 'min7', 2)).toEqual([64, 67, 69, 72]);
      expect(chordPitches(57, 'min7', 3)).toEqual([67, 69, 72, 76]);
      // a two-note voicing has one inversion; a triad has two
      expect(chordPitches(60, '5', 3)).toEqual([67, 72]);
      expect(chordPitches(60, 'maj', 5)).toEqual([67, 72, 76]);
      // the MIDI limit holds after rotating: lifting the root overflows (CodeRabbit)
      expect(chordPitches(118, 'maj', 0)).toEqual([118, 122, 125]);
      expect(chordPitches(118, 'maj', 1)).toBeNull();
    });

    it('naming is bass-first (a root-position C6 is C6, never Am7)', () => {
      expect(chordName([60, 64, 67, 69])).toBe('C6');
      expect(chordName([57, 60, 64, 67])).toBe('Am7');
      expect(chordName([60, 63, 66, 69])).toBe('C°7');
      expect(chordName([60, 63, 66, 70])).toBe('Cm7♭5');
      expect(chordName([60, 64, 67, 70, 74])).toBe('C9');
      expect(chordName([60, 61])).toBe('C?');
    });

    it('the progression template table matches the engine', () => {
      expect(PROGRESSION_TEMPLATES).toHaveLength(9);
      expect(progressionDegrees('pop', 6, 0, 7)).toEqual([0, 4, 5, 3, 0, 4]);
      expect(progressionDegrees('canon', 10, 0, 7)).toEqual([0, 4, 5, 2, 3, 0, 3, 4, 0, 4]);
      expect(progressionDegrees('nope', 4, 0, 7)).toBeNull();
    });

    it('chance walk matches the engine bit for bit (golden degrees)', () => {
      expect(progressionDegrees('chance', 4, 2026, 7)).toEqual([2, 4, 3, 2]);
      expect(progressionDegrees('chance', 4, 7, 7)).toEqual(progressionDegrees('chance', 4, 7, 7));
      expect(progressionDegrees('chance', 4, 7, 7)).not.toEqual(progressionDegrees('chance', 4, 8, 7));
    });

    it('progression golden voicings: minloop voice-lead off and on', () => {
      const off: Note[] = [];
      applyTransform(off, { kind: 'progression', ch: 1, bars: 4, template: 'minloop', voiceLead: false }, 0, null, roots, AMINOR.key, AMINOR.scale, 512);
      expect(off.map(n => n.p)).toEqual([57, 60, 64, 67, 53, 57, 60, 64, 48, 52, 55, 59, 55, 59, 62, 65]);
      expect(off.map(n => n.step)).toEqual([0, 0, 0, 0, 16, 16, 16, 16, 32, 32, 32, 32, 48, 48, 48, 48]);
      const on: Note[] = [];
      applyTransform(on, { kind: 'progression', ch: 1, bars: 4, template: 'minloop', voiceLead: true }, 0, null, roots, AMINOR.key, AMINOR.scale, 512);
      expect(on.map(n => n.p)).toEqual([57, 60, 64, 67, 57, 60, 64, 65, 59, 60, 64, 67, 59, 62, 65, 67]);
    });

    it('progression golden: chance seed 2026, voice-lead on', () => {
      const a: Note[] = [];
      applyTransform(a, { kind: 'progression', ch: 1, bars: 4, template: 'chance', voiceLead: true }, 2026, null, roots, AMINOR.key, AMINOR.scale, 512);
      expect(a.map(n => n.p)).toEqual([48, 52, 55, 59, 52, 55, 59, 62, 53, 57, 60, 62, 52, 55, 59, 60]);
      expect(a.map(n => n.step)).toEqual([0, 0, 0, 0, 16, 16, 16, 16, 32, 32, 32, 32, 48, 48, 48, 48]);
      // determinism: same seed, byte-identical; other seed, different
      const b: Note[] = [];
      applyTransform(b, { kind: 'progression', ch: 1, bars: 4, template: 'chance', voiceLead: true }, 2027, null, roots, AMINOR.key, AMINOR.scale, 512);
      expect(a).not.toEqual(b);
    });

    it('progression replaces the channel and keeps the others', () => {
      const a: Note[] = [note(1, 2, 40, 1), note(2, 5, 70, 1)];
      applyTransform(a, { kind: 'progression', ch: 1, bars: 4, template: 'minloop', voiceLead: false }, 0, null, [{ id: 1, root: 60 }, { id: 2, root: 60 }], AMINOR.key, AMINOR.scale, 512);
      expect(a.filter(n => n.ch === 1)).toHaveLength(16);
      expect(a.some(n => n.ch === 2 && n.step === 5)).toBe(true);
    });

    it('progression validates like the engine', () => {
      const run = (verb: Parameters<typeof applyTransform>[1]) => {
        const a: Note[] = [];
        expect(() => applyTransform(a, verb, 0, null, roots, AMINOR.key, AMINOR.scale, 512)).toThrow();
      };
      run({ kind: 'progression', ch: 1, bars: 33, template: 'pop' });
      run({ kind: 'progression', ch: 1, bars: 4, template: 'nope' });
      run({ kind: 'progression', ch: 1, degrees: [33] });
      run({ kind: 'progression', ch: 1, degrees: [0.5] }); // fractional degrees are invalid
      run({ kind: 'progression', ch: 1, degrees: [0, 1, 2, 3, 4, 5, 6, 7] }); // degree 7 does not exist here
      run({ kind: 'progression', ch: 1, degrees: Array.from({ length: 33 }, (_, i) => i % 7) }); // > 32 entries
      run({ kind: 'progression', ch: 999, bars: 4, template: 'pop' });
      // a scale-less project cannot spell diatonic stacks
      const a: Note[] = [];
      expect(() => applyTransform(a, { kind: 'progression', ch: 1, bars: 4, template: 'pop' }, 0, null, roots, null, 1, 512)).toThrow();
    });

    it('chord verb harmonizes diatonic single notes and skips out-of-scale roots', () => {
      const a: Note[] = [{ ch: 1, step: 0, len: 2, p: 52, v: 0.8 }]; // E3
      applyTransform(a, { kind: 'chord', harmony: 'diatonic' }, 0, null, roots, AMINOR.key, AMINOR.scale, 512);
      expect(a.map(n => n.p)).toEqual([52, 55, 59]); // Em
      const b: Note[] = [{ ch: 1, step: 0, len: 1, p: 61, v: 0.8 }]; // C#4: not in A minor
      applyTransform(b, { kind: 'chord', harmony: 'diatonic' }, 0, null, roots, AMINOR.key, AMINOR.scale, 512);
      expect(b).toHaveLength(1); // left alone
    });

    it('chord verb spells named types by interval anywhere', () => {
      const a: Note[] = [{ ch: 1, step: 0, len: 1, p: 49, v: 0.8 }]; // C#3
      applyTransform(a, { kind: 'chord', harmony: 'min7' }, 0, null, roots, AMINOR.key, AMINOR.scale, 512);
      expect(a.map(n => n.p)).toEqual([49, 52, 56, 59]); // C#m7
    });

    it('chord verb does not double a tone another channel sounds', () => {
      const a: Note[] = [{ ch: 1, step: 0, len: 2, p: 52, v: 0.8 }, { ch: 2, step: 0, len: 2, p: 59, v: 0.8 }];
      applyTransform(a, { kind: 'chord', harmony: 'diatonic' }, 0, null, [{ id: 1, root: 60 }, { id: 2, root: 60 }], AMINOR.key, AMINOR.scale, 512);
      expect(a).toHaveLength(5);
      expect(a.filter(n => n.p === 59)).toHaveLength(1);
    });

    it('chord verb dedupe sees notes outside the selection', () => {
      // E3 selected; B3 on another channel at the same step, OUTSIDE the selection —
      // the duplicate check runs against the whole pattern, not just the target
      const a: Note[] = [
        { ch: 1, step: 4, len: 2, p: 52, v: 0.8 },
        { ch: 2, step: 4, len: 2, p: 59, v: 0.8 },
        { ch: 1, step: 8, len: 2, p: 60, v: 0.8 },
      ];
      applyTransform(a, { kind: 'chord', harmony: 'diatonic' }, 0, { kind: 'range', from: 4, to: 9, ch: 1 }, [{ id: 1, root: 60 }, { id: 2, root: 60 }], AMINOR.key, AMINOR.scale, 512);
      expect(a.filter(n => n.p === 59)).toHaveLength(1);
      expect(a.filter(n => n.p === 55)).toHaveLength(1); // E3 still gains its G
    });

    it('chord verb dedupe resolves implicit channel roots', () => {
      // ch 2 has p: undefined with root 59 (B3) — no explicit B3 may be added
      const a: Note[] = [
        { ch: 1, step: 0, len: 2, p: 52, v: 0.8 },
        { ch: 2, step: 0, len: 2, v: 0.8 },
      ];
      applyTransform(a, { kind: 'chord', harmony: 'diatonic' }, 0, null, [{ id: 1, root: 60 }, { id: 2, root: 59 }], AMINOR.key, AMINOR.scale, 512);
      expect(a.filter(n => n.p === 59)).toHaveLength(0);
      expect(a).toHaveLength(5);
    });

    it('chord verb validates like the engine', () => {
      const run = (verb: Parameters<typeof applyTransform>[1], key: number | null) => {
        const a: Note[] = [{ ch: 1, step: 0, len: 1, p: 60, v: 0.8 }];
        expect(() => applyTransform(a, verb, 0, null, roots, key, 2, 512)).toThrow();
      };
      run({ kind: 'chord', harmony: 'm7b5' }, 9); // stamp-only type
      run({ kind: 'chord', harmony: 'diatonic' }, null); // no key
    });

    it('golden: chord verb on a selected range merges like the engine', () => {
      const a: Note[] = [{ ch: 1, step: 0, len: 1, p: 61, v: 0.8 }, { ch: 1, step: 4, len: 2, p: 52, v: 0.8 }];
      applyTransform(a, { kind: 'chord', harmony: 'diatonic' }, 0, { kind: 'range', from: 4, to: 9, ch: 1 }, roots, AMINOR.key, AMINOR.scale, 512);
      // step 0 out of range (and out of scale anyway); E3 → Em
      expect(a.map(n => n.p)).toEqual([61, 52, 55, 59]);
    });
  });
});
