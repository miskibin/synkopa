// The pointer-tool layer's first tests: the dispatch chain (which gesture a click starts,
// under which modifiers and hit) and the gestures' data flow, driven through a fake RollCtx —
// no DOM, no backend, exactly the seam the refactor (deep-5) was after.
import { describe, expect, it } from 'vitest';
import type { Channel, Note, Op, Pattern } from '../../api/types';
import { noteAt, noteWidth, snapStep } from '../../lib/pattern';
import { chordPitches } from '../../lib/transforms';
import { rollDown, rollMove } from './grid';
import { KW, type Mods, type Pt, type RollCtx, type RollMove, type Tool } from './types';

// ---- fake geometry and the fake context ----

const CH = { id: 3, name: 'Lead', color: '#38a', root: 60, mute: false } as unknown as Channel;
const GEO = { pps: 16, sx: 0, sy: 0, rowH: 18, kw: KW, steps: 64 };
const note = (step: number, key: number, len = 4, ch = CH.id, v?: number): Note => ({ ch, step, len, p: key, ...(v != null ? { v } : {}) });

/** Grid position for (step, key) in the fake geometry (1 step = one sixteenth). */
const at = (step: number, key: number, dx = 0): Pt => ({ x: KW + step * GEO.pps + dx, y: (127 - key) * GEO.rowH, step, key });

interface RecGest {
  label: string; opss: Note[][]; ended: number;
  update(ops: Op[]): void; end(): void;
}
interface RecDrag { o: { frame?: boolean; cursor?: string }; move(m: RollMove): void; up(m: RollMove): void }

/** The last update's notes (the fake gesture's ops carry putPattern with the full notes). */
const notesOf = (g: RecGest) => g.opss.at(-1) ?? [];

function makeCtx(notes: Note[], tool: Tool = 'draw') {
  const rec: {
    says: string[]; sels: number[][]; commits: { notes: Note[]; label: string }[];
    gestures: RecGest[]; auditions: { key: number; vel: number }[]; typing: { on: boolean; key: number; vel: number }[];
    switched: number[]; props: number[]; zooms: { s0: number; s1: number; r0: number; r1: number }[];
    pushed: number; popped: number; drags: number; marq: unknown; ghost: unknown; chord: string;
    propsOpen: boolean; stampType: string;
  } = {
    says: [], sels: [], commits: [], gestures: [], auditions: [], typing: [],
    switched: [], props: [], zooms: [], pushed: 0, popped: 0, drags: 0, marq: null, ghost: null, chord: '', propsOpen: false,
    stampType: 'min',
  };
  const pat = { id: 7, name: 'P', color: '#fff', len: 64, notes } as unknown as Pattern;
  // the tools replace the whole notes array per op; the component's store does it at once —
  // the fake does the same so hit tests see each edit, like the real roll does. The length
  // mirrors the engine's fit_len (model.rs): the last note end in beats, rounded up to a
  // whole bar, but never below the length the user kept on purpose.
  const live = notes.slice();
  const setNotes = (nts: readonly Note[]) => {
    live.splice(0, live.length, ...nts);
    const end = live.reduce((m: number, n: Note) => Math.max(m, n.step + n.len), 0);
    const content = Math.ceil(end / 16 - 1e-6) * 4;
    pat.len = Math.max(pat.minLen ?? 4, content);
  };
  let sel = new Set<number>();
  let lastLen = 2, lastVel = 1;
  let drag: RecDrag | null = null;
  let dblArmed = false;
  let modCb: ((m: Mods) => void) | null = null;
  const ctx: RollCtx = {
    pt: () => pat,
    ch: () => CH,
    notes: () => live,
    chanNotes: () => live.map((n, i) => ({ n, i })).filter(x => x.n.ch === CH.id),
    chans: () => new Map([[CH.id, CH]]),
    geo: () => GEO,
    snap: () => 1,
    hit: (s, k) => noteAt(live, CH.id, CH, s, k, GEO.pps),
    edgeHit: (i, x) => {
      const n = live[i], G = GEO;
      const width = noteWidth(n, G.pps);
      return x >= G.kw + n.step * G.pps + width - Math.min(6, width * 0.4);
    },
    ghostAt: (s, k) => {
      for (let i = live.length - 1; i >= 0; i--) {
        const n = live[i]; if (n.ch === CH.id) continue;
        if (n.p === k && s >= n.step - 1e-3 && s < n.step + noteWidth(n, GEO.pps) / GEO.pps) return i;
      }
      return -1;
    },
    tool: () => tool,
    sel: () => sel,
    setSel: s => { sel = s; rec.sels.push([...s]); },
    lastLen: () => lastLen, setLastLen: l => { lastLen = l; },
    lastVel: () => lastVel, setLastVel: v => { lastVel = v; },
    commitNotes: (nts, label) => { setNotes(nts); rec.commits.push({ notes: nts, label }); },
    notesOp: nts => {
      setNotes(nts);
      return { op: 'putPattern' as const, pattern: { ...pat, notes: [...nts] } };
    },
    gest: label => {
      const g: RecGest = {
        label, opss: [], ended: 0,
        update(ops) { g.opss.push(ops.flatMap(o => (o.op === 'putPattern' ? (o.pattern as { notes: Note[] }).notes : []))); },
        end() { g.ended++; },
      };
      rec.gestures.push(g);
      return g;
    },
    audition: (key, vel) => rec.auditions.push({ key, vel }),
    typingNote: (on, key, vel) => rec.typing.push({ on, key, vel }),
    say: msg => rec.says.push(msg),
    setMarq: m => { rec.marq = m; },
    setZoomBox: () => {},
    setStampGhost: g => { rec.ghost = g; },
    stampGhost: () => null,
    setHoverChord: s => { rec.chord = s; },
    setHoverKey: () => {},
    propsOpen: () => rec.propsOpen, closeProps: () => { rec.propsOpen = false; }, openProps: idx => { rec.props = idx; },
    stamp: () => ({ type: rec.stampType, inv: 0 }),
    keyScale: () => ({ root: 0, scale: 1 }),
    switchChannel: id => rec.switched.push(id),
    zoomTo: (s0, s1, r0, r1) => rec.zooms.push({ s0, s1, r0, r1 }),
    pushView: () => { rec.pushed++; },
    popView: () => (rec.popped < rec.pushed ? (rec.popped++, { sx: 1, sy: 1, zoom: 1, rowH: 18 }) : undefined),
    applyView: () => {},
    dbl: () => { const d = dblArmed; dblArmed = false; return d; },
    pan: () => false,
    setDragging: on => { if (on) rec.drags++; },
    beginDrag: (o, move, up) => { drag = { o, move, up }; },
    bindMods: cb => { modCb = cb; return () => { modCb = null; }; },
    autoScroll: () => ({ move: () => {}, stop: () => {} }),
  };
  return { ctx, rec, live, drag: () => drag, armDbl: () => { dblArmed = true; }, fireMods: (m: Mods) => modCb?.(m), setLen: (n: number) => { pat.len = n; pat.minLen = n; }, patLen: () => pat.len };
}

// ---- helpers over the fakes ----

const down = (c: ReturnType<typeof makeCtx>, button: 0 | 2, p: Pt, mods: Partial<Mods> = {}) =>
  rollDown(c.ctx, { button, alt: !!mods.alt, shift: !!mods.shift, ctrl: !!mods.ctrl, p });

const mv = (c: ReturnType<typeof makeCtx>, p: Pt, mods: Partial<Mods> = {}): RollMove =>
  ({ alt: !!mods.alt, shift: !!mods.shift, ctrl: !!mods.ctrl, p });

describe('draw tool', () => {
  it('creates a snapped note on empty grid, selects it, auditions it, and drags it', () => {
    const c = makeCtx([]);
    down(c, 0, at(4.4, 60));
    const g = c.rec.gestures[0];
    expect(g.label).toBe('New note');
    expect(notesOf(g)).toEqual([{ ch: CH.id, step: 4, len: 2, p: 60, trig: false }]);
    expect(c.rec.sels.at(-1)).toEqual([0]);
    expect(c.rec.auditions).toEqual([{ key: 60, vel: 1 }]);
    // the create hands its gesture to the move drag: a real move nudges, up closes the undo step
    c.drag()?.move(mv(c, at(4.4, 60)));
    c.drag()?.move(mv(c, at(6.4, 60)));
    expect(notesOf(g).at(-1)).toEqual({ ch: CH.id, step: 6, len: 2, p: 60, trig: false });
    c.drag()?.up(mv(c, at(6.4, 60)));
    expect(g.ended).toBe(1);
  });

  it('Alt places the note free of the grid', () => {
    const c = makeCtx([]);
    down(c, 0, at(4.4, 60), { alt: true });
    expect(notesOf(c.rec.gestures[0])[0].step).toBeCloseTo(snapStep(4.4, 0, true), 5);
  });

  it('a new note past the end grows the pattern and says so', () => {
    const c = makeCtx([]);
    c.setLen(16);
    down(c, 0, at(66.4, 60)); // past the 4-bar pattern: the note ends at step 68 = beat 17
    expect(c.patLen()).toBe(20); // → five bars
    expect(c.rec.says.at(-1)).toBe('Pattern grew to 5 bars');
  });

  it('a click on a note moves it with its drag; the right edge resizes it', () => {
    const c = makeCtx([note(4, 60)]);
    // the note's body: a move drag (the lead note is auditioned)
    down(c, 0, at(5, 60));
    c.drag()?.move(mv(c, at(8, 60)));
    expect(c.live[0].step).toBe(7); // moveNotes snaps the first landing to the grid
    c.drag()?.up(mv(c, at(8, 60)));
    // the drag-right edge (the last 6 px of the note): a resize drag
    const c2 = makeCtx([note(4, 60, 8)]);
    down(c2, 0, at(11.8, 60));
    c2.drag()?.move(mv(c2, at(13, 60)));
    expect(c2.live[0].len).toBe(9); // the end snaps to the grid, length follows
    c2.drag()?.up(mv(c2, at(13, 60)));
    expect(c2.rec.gestures[0].label).toBe('Note length');
  });

  it('the paint tool keeps writing notes cell after cell, once per cell', () => {
    const c = makeCtx([], 'paint');
    down(c, 0, at(4.4, 60));
    expect(c.live.map(n => n.step)).toEqual([4]);
    c.drag()?.move(mv(c, at(8.4, 60))); // next cell: another note
    expect(c.live.map(n => n.step)).toEqual([4, 8]);
    c.drag()?.move(mv(c, at(8.6, 60))); // still cell 8: no duplicate
    expect(c.live).toHaveLength(2);
    c.drag()?.up(mv(c, at(8.6, 60)));
    expect(c.rec.gestures[0].label).toBe('Paint notes');
    expect(c.rec.gestures[0].ended).toBe(1);
  });

  it('a modifier pressed mid-drag locks an axis until it changes again (FL)', () => {
    const c = makeCtx([note(4, 60)]);
    down(c, 0, at(5, 60));
    c.drag()?.move(mv(c, at(8, 62))); // free move: snaps to step 7, pitch follows
    expect(c.live[0]).toMatchObject({ step: 7, p: 62 });
    // Shift pressed after the drag started: pitch locks, time follows — applied at once
    // even without a pointer move (the key-only modifier path)
    c.fireMods({ alt: false, shift: true, ctrl: false });
    expect(c.rec.says).toContain('Moving in time only (Shift)');
    c.drag()?.move(mv(c, at(12, 70), { shift: true }));
    // locked moves recompute from the drag-start position: the pitch returns to it
    expect(c.live[0]).toMatchObject({ step: 11, p: 60 });
    // Ctrl instead: time locks to the drag-start step, pitch follows
    c.drag()?.move(mv(c, at(16, 76), { ctrl: true }));
    expect(c.live[0]).toMatchObject({ step: 4, p: 76 });
    expect(c.rec.says).toContain('Moving in pitch only (Ctrl)');
    c.drag()?.up(mv(c, at(16, 76), { ctrl: true }));
    expect(c.rec.gestures[0].ended).toBe(1);
  });
});

describe('erase', () => {
  it('right-click deletes the note under the pointer, one undo step, selection cleared', () => {
    const c = makeCtx([note(4, 60), note(10, 62)]);
    down(c, 2, at(5, 60));
    c.drag()?.up(mv(c, at(5, 60)));
    const g = c.rec.gestures[0];
    expect(g.label).toBe('Delete notes');
    expect(c.live.map(n => n.step)).toEqual([10]);
    expect(g.ended).toBe(1);
    expect(c.rec.says.some(s => s.startsWith('Deleted'))).toBe(true);
  });

  it('the erase tool sweeps per note and clears the selection at once', () => {
    const c = makeCtx([note(4, 60), note(8, 62)], 'erase');
    down(c, 0, at(4.5, 60));
    c.drag()?.move(mv(c, at(8.5, 62)));
    c.drag()?.up(mv(c, at(8.5, 62)));
    expect(c.live).toEqual([]);
    expect(c.rec.gestures[0].ended).toBe(1);
  });
});

describe('stamp tool', () => {
  it('a click writes the chord rooted at the key, selects it, auditions every voice', () => {
    const c = makeCtx([], 'stamp');
    down(c, 0, at(4.4, 60));
    const pitches = chordPitches(60, 'min', 0)!;
    expect(c.rec.commits).toHaveLength(1);
    expect(c.rec.commits[0].notes.filter(n => n.ch === CH.id)).toEqual(pitches.map(p => ({ ch: CH.id, step: 4, len: 2, p: p, trig: false })));
    expect(c.rec.sels.at(-1)).toEqual([0, 1, 2]);
    expect(c.rec.auditions.map(a => a.key)).toEqual(pitches);
    expect(c.rec.says.some(s => s.includes('m'))).toBe(true);
  });

  it('re-stamping a step replaces that chord only; other steps and channels stay', () => {
    const ghost = note(4, 40, 2, 9); // another channel's note at the same step
    const c = makeCtx([note(4, 60), note(4, 63), note(4, 67), note(12, 64), ghost], 'stamp');
    down(c, 0, at(4.2, 64));
    const steps = c.live.map(n => `${n.ch}:${n.step}:${n.p}`);
    expect(steps.filter(s => s.startsWith('3:4'))).toEqual(['3:4:64', '3:4:67', '3:4:71']); // Dm over the old Cm
    expect(steps).toContain('9:4:40'); // the ghost untouched
    expect(steps).toContain('3:12:64'); // the other step untouched
  });

  it('right-drag erases whole chords per step, once per step', () => {
    const c = makeCtx([note(4, 60), note(4, 63), note(4, 67), note(8, 62)], 'stamp');
    down(c, 2, at(4.5, 60));
    c.drag()?.move(mv(c, at(4.9, 62))); // same step as the click: no second update
    expect(c.rec.gestures[0].opss).toHaveLength(1);
    c.drag()?.move(mv(c, at(8.5, 62))); // next step: the chord there goes too
    expect(c.rec.gestures[0].opss).toHaveLength(2);
    c.drag()?.up(mv(c, at(8.5, 62)));
    expect(c.live.map(n => n.step)).toEqual([]);
    expect(c.rec.gestures[0].ended).toBe(1);
  });

  it('stamping past the pattern end grows it and the stamp message carries the notice', () => {
    const c = makeCtx([], 'stamp');
    c.setLen(16);
    down(c, 0, at(68.4, 60)); // past the 4-bar pattern: the stamp ends at step 70 = beat 17.5
    expect(c.patLen()).toBe(20); // → five bars
    expect(c.rec.says.at(-1)).toBe('Stamped Cm · Pattern grew to 5 bars'); // one status line, both facts
  });

  it('a stamp inside the pattern says nothing about growth', () => {
    const c = makeCtx([], 'stamp');
    c.setLen(16);
    down(c, 0, at(4.4, 60));
    expect(c.rec.says.at(-1)).toBe('Stamped Cm'); // no growth, no notice
  });
});

describe('modifiers and the shared chain', () => {
  it('Ctrl+click toggles one note with any tool; Ctrl+drag on empty marquees', () => {
    const c = makeCtx([note(4, 60), note(8, 62)]);
    down(c, 0, at(5, 60), { ctrl: true });
    expect(c.rec.sels.at(-1)).toEqual([0]);
    down(c, 0, at(5, 60), { ctrl: true }); // again: toggles off
    expect(c.rec.sels.at(-1)).toEqual([]);
    down(c, 0, at(20, 60), { ctrl: true }); // empty grid: a marquee drag starts
    expect(c.drag()).toBeTruthy();
    expect(c.rec.gestures).toHaveLength(0); // a marquee is not an edit
  });

  it('Shift+click clones (copies on the first real move, originals on a click)', () => {
    const c = makeCtx([note(4, 60)]);
    down(c, 0, at(5, 60), { shift: true });
    expect(c.rec.gestures).toHaveLength(0); // nothing yet: the drag has not moved
    c.drag()?.move(mv(c, at(9, 62)));
    expect(c.rec.gestures[0].label).toBe('Clone notes');
    expect(c.live).toHaveLength(2);
    expect(c.live[1]).toEqual({ ch: CH.id, step: 8, len: 4, p: 62 }); // a copy of the original, moved
    c.drag()?.up(mv(c, at(9, 62)));
    // a second Shift+click without moving: just toggles the note in the selection
    down(c, 0, at(9.5, 62), { shift: true });
    c.drag()?.up(mv(c, at(9.5, 62)));
    expect(c.rec.sels.at(-1)).toEqual([]); // the clone had selected the copy; the click toggles it off
  });

  it('Shift+drag on empty adds a marquee to the selection; a click without moving keeps the clone pending', () => {
    const c = makeCtx([note(4, 60), note(8, 62)]);
    down(c, 0, at(1, 40), { shift: true });
    c.drag()?.move(mv(c, at(15, 70)));
    c.drag()?.up(mv(c, at(15, 70)));
    expect(c.rec.sels.at(-1)).toEqual([0, 1]); // both notes in the box, added
  });

  it('a double-click opens the note properties with the clicked note last', () => {
    const c = makeCtx([note(4, 60), note(8, 62)]);
    c.ctx.sel(); // no selection: props for the clicked one
    c.armDbl();
    down(c, 0, at(5, 60));
    expect(c.rec.props).toEqual([0]);
    // with a selection, the clicked note moves last
    const c2 = makeCtx([note(4, 60), note(8, 62)]);
    c2.ctx.setSel(new Set([0, 1]));
    c2.armDbl();
    down(c2, 0, at(9, 62));
    expect(c2.rec.props).toEqual([0, 1]);
  });

  it('a double right-click on a ghost note switches to that channel', () => {
    const c = makeCtx([note(4, 60), note(8, 40, 4, 9)]);
    c.armDbl();
    down(c, 2, at(9, 40));
    expect(c.rec.switched).toEqual([9]);
  });

  it('the keyboard lane: a plain click plays and glides, Ctrl+click selects a pitch', () => {
    const c = makeCtx([note(4, 60), note(8, 60)]);
    down(c, 0, { x: 10, y: (127 - 60) * 18, step: -2.4, key: 60 });
    expect(c.rec.typing[0]).toEqual({ on: true, key: 60, vel: expect.any(Number) });
    expect(c.drag()).toBeTruthy(); // the glide drag is open
    c.drag()?.up(mv(c, { x: 10, y: (127 - 60) * 18, step: -2.4, key: 60 }));
    expect(c.rec.typing.at(-1)).toEqual({ on: false, key: 60, vel: 0 });
    down(c, 0, { x: 10, y: (127 - 60) * 18, step: -2.4, key: 60 }, { ctrl: true });
    expect(c.rec.sels.at(-1)).toEqual([0, 1]); // every note of that pitch
  });

  it('Ctrl+right-drag zooms to a box; a bare Ctrl+right-click goes back a view', () => {
    const c = makeCtx([]);
    down(c, 2, at(8, 60), { ctrl: true });
    c.drag()?.move(mv(c, at(24, 72)));
    c.drag()?.up(mv(c, at(24, 72)));
    expect(c.rec.pushed).toBe(1);
    expect(c.rec.zooms[0].s0).toBeCloseTo(8, 5); // content units: steps across, rows down
    expect(c.rec.zooms[0].s1).toBeCloseTo(24, 5);
    // without a drag and nothing to go back to: the status line says so
    const c2 = makeCtx([]);
    down(c2, 2, at(8, 60), { ctrl: true });
    c2.drag()?.up(mv(c2, at(8.05, 60)));
    expect(c2.rec.says).toContain('No previous view');
  });
});

describe('hover', () => {
  it('cursors: the keyboard lane points, tools speak, Ctrl crosshairs, the stamp previews', () => {
    const notes = [note(4, 60, 8)];
    expect(rollMove(makeCtx(notes).ctx, mv(makeCtx(notes), { x: 10, y: 0, step: -2, key: 60 }))).toBe('pointer');
    const overBody = mv(makeCtx(notes), at(5, 60));
    expect(rollMove(makeCtx(notes).ctx, overBody)).toBe('grab');
    const overEdge = mv(makeCtx(notes), at(11.8, 60));
    expect(rollMove(makeCtx(notes).ctx, overEdge)).toBe('ew-resize');
    expect(rollMove(makeCtx(notes, 'erase').ctx, mv(makeCtx(notes), at(5, 60)))).toBe('not-allowed');
    expect(rollMove(makeCtx(notes, 'select').ctx, mv(makeCtx(notes), at(30, 30)))).toBe('crosshair');
    expect(rollMove(makeCtx(notes, 'stamp').ctx, mv(makeCtx(notes), at(30, 30)))).toBe('copy');
    expect(rollMove(makeCtx(notes).ctx, mv(makeCtx(notes), at(5, 60), { ctrl: true }))).toBe('crosshair');
  });

  it('the stamp hover sets the ghost for the snapped cell and the status preview', () => {
    const c = makeCtx([], 'stamp');
    const cursor = rollMove(c.ctx, mv(c, at(4.4, 60)));
    expect(cursor).toBe('copy');
    expect(c.rec.ghost).toEqual({ s: 4, len: 2, pitches: chordPitches(60, 'min', 0) });
    expect(c.rec.chord).toBe('C4 → Cm · in C Major'); // root, the resulting chord, the in-scale marker
    // off the MIDI range for the chord: no ghost, no crash
    rollMove(c.ctx, mv(c, at(6.4, 127)));
    expect(c.rec.chord).toBe('');
  });

  it('a chord under the pointer names it in the status line', () => {
    const c = makeCtx([note(4, 60), note(4, 63), note(4, 67), note(12, 64)]);
    rollMove(c.ctx, mv(c, at(5, 63)));
    expect(c.rec.chord).toBe('Cm');
    rollMove(c.ctx, mv(c, at(13, 64)));
    expect(c.rec.chord).toBe(''); // a single note is no chord
  });

  it('hover previews keep updating while Ctrl is held; only the cursor is the crosshair', () => {
    const c = makeCtx([note(4, 60), note(4, 63), note(4, 67)], 'stamp');
    rollMove(c.ctx, mv(c, at(4.4, 60), { ctrl: true }));
    expect(c.rec.ghost).toEqual({ s: 4, len: 2, pitches: chordPitches(60, 'min', 0) });
    expect(c.rec.chord).toBe('C4 → Cm · in C Major');
  });

  it('the ghost follows a chord-type change even when the new pitches extend the old', () => {
    const c = makeCtx([], 'stamp');
    rollMove(c.ctx, mv(c, at(4.4, 60))); // Cm triad: [60, 63, 67]
    expect(c.rec.ghost).toEqual({ s: 4, len: 2, pitches: [60, 63, 67] });
    c.rec.stampType = 'min7'; // extends the triad: [60, 63, 67, 70]
    rollMove(c.ctx, mv(c, at(4.4, 60)));
    expect(c.rec.ghost).toEqual({ s: 4, len: 2, pitches: [60, 63, 67, 70] });
  });
});

describe('select tool', () => {
  it('a drag on a note moves it with its selection', () => {
    const c = makeCtx([note(4, 60), note(8, 62)], 'select');
    down(c, 0, at(5, 60));
    c.drag()?.move(mv(c, at(8, 60)));
    expect(c.live[0]).toMatchObject({ step: 7, p: 60 }); // moveNotes snaps the landing
    expect(c.rec.sels.at(-1)).toEqual([0]);
    c.drag()?.up(mv(c, at(8, 60)));
    expect(c.rec.gestures[0].ended).toBe(1);
  });

  it('empty grid starts the marquee and the box selects the notes it covers', () => {
    const c = makeCtx([note(4, 60)], 'select');
    down(c, 0, at(1, 40));
    expect(c.drag()).toBeTruthy();
    c.drag()?.move(mv(c, at(15, 70)));
    c.drag()?.up(mv(c, at(15, 70)));
    expect(c.rec.sels.at(-1)).toEqual([0]);
    expect(c.rec.gestures).toHaveLength(0); // a marquee is not an edit
  });
});

describe('keyboard lane glide', () => {
  it('gliding re-triggers the key under the pointer and releases on up', () => {
    const c = makeCtx([]);
    down(c, 0, { x: 10, y: (127 - 60) * 18, step: -2.4, key: 60 });
    c.drag()?.move(mv(c, { x: 20, y: (127 - 64) * 18, step: -2.4, key: 64 }));
    expect(c.rec.typing).toEqual([
      { on: true, key: 60, vel: expect.any(Number) },
      { on: false, key: 60, vel: 0 },
      { on: true, key: 64, vel: expect.any(Number) },
    ]);
    c.drag()?.up(mv(c, { x: 20, y: (127 - 64) * 18, step: -2.4, key: 64 }));
    expect(c.rec.typing.at(-1)).toEqual({ on: false, key: 64, vel: 0 });
  });
});




