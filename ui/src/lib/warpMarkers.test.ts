// Warp markers on audio clips: the map math, the editors that keep it consistent (stretch,
// fit), and the op path through the mock (sorted storage, stripped when emptied, rejections).
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { MockBackend } from '../api/mock';
import { applyAll, OpError } from './applyOps';
import { type Clip, type WarpPoint } from '../api/types';
import { fitClip, markersAtTransients, moveWarp, pinWarp, removeWarp, stretchClip, warpActive, warpDragBounds, warpedFrom, warpedTo } from './clipOps';

const marker = (at: number, to: number): WarpPoint => ({ at, to });

const clip = (p: Partial<Clip> = {}): Clip => ({ id: 5, track: 0, start: 8, len: 4, offset: 0, kind: 'audio', ref: 10, muted: false, gain: 0, fadeIn: 0, fadeOut: 0, pan: 0, norm: false, rev: false, ...p });

beforeAll(() => { vi.stubGlobal('window', {}); });

describe('the warp map', () => {
  it('maps at the baseline with no markers', () => {
    const c = clip({ stretch: 2 });
    expect(warpedTo(c, 1.5, 4)).toBe(3);
    expect(warpedFrom(c, 3, 4)).toBe(1.5);
    expect(warpActive(c)).toBe(false);
  });

  it('pins a moment and keeps the ends', () => {
    const c = clip({ warp: [marker(1, 1.4)] });
    expect(warpActive(c)).toBe(true);
    expect(warpedTo(c, 1, 2)).toBe(1.4);
    expect(warpedTo(c, 0, 2)).toBe(0);
    expect(warpedTo(c, 2, 2)).toBeCloseTo(2, 9);
    expect(warpedFrom(c, 1.4, 2)).toBeCloseTo(1, 9);
    // the inverse at a moment the map did not pin
    expect(warpedFrom(c, warpedTo(c, 0.7, 2), 2)).toBeCloseTo(0.7, 6);
  });

  it('a resampled clip never warps', () => {
    expect(warpActive(clip({ resample: true, warp: [marker(1, 1)] }))).toBe(false);
  });
});

describe('editing the map', () => {
  it('pins sorted, landing where the current map puts the moment', () => {
    const c = clip({ warp: [marker(1, 2)] });
    const p = pinWarp(c, 1.5, 2)!;
    expect(p.warp!.map(w => w.at)).toEqual([1, 1.5]);
    expect(p.warp![1].to).toBe(warpedTo(c, 1.5, 2));
    expect(pinWarp(c, 1, 2)).toBeNull(); // the spot is taken
    expect(pinWarp(clip({ warp: Array.from({ length: 1024 }, (_, i) => marker(i * 0.0024, i * 0.0024)) }), 5, 6)).toBeNull();
  });

  it('pins onto a clip that never had markers', () => {
    const p = pinWarp(clip(), 1, 2)!;
    expect(p.warp).toEqual([marker(1, 1)]);
    expect(warpActive(p)).toBe(true);
  });

  it('moves and removes', () => {
    const c = clip({ warp: [marker(1, 1), marker(2, 2)] });
    expect(moveWarp(c, 1, 3).warp![1].to).toBe(3);
    expect(removeWarp(c, 0).warp).toEqual([marker(2, 2)]);
    expect(removeWarp(c, 5).warp).toHaveLength(2);
  });

  it('pins markers at transients at their mapped places, skipping the too-close ones', () => {
    const c = clip({ warp: [marker(0.5, 0.5)] });
    const m = markersAtTransients(c, [0.1, 0.5000001, 1.2, 1.2000001], 2)!;
    expect(m.warp!.map(w => w.at)).toEqual([0.1, 0.5, 1.2]);
    // the new pin lands where the old map put 1.2 s of source: pinning never jumps the audio
    expect(m.warp![2].to).toBe(warpedTo(c, 1.2, 2));
  });

  it('pinning at transients never moves an existing marker', () => {
    // the review's case: a transient 4 ms before a pinned marker used to filter the PIN away
    const c = clip({ warp: [marker(0.5, 1.5)] });
    const m = markersAtTransients(c, [0.496, 1.4], 2)!;
    expect(m.warp).toEqual([marker(0.5, 1.5), marker(1.4, warpedTo(c, 1.4, 2))]);
    // the pinned moment still lands exactly where it was pinned
    expect(m.warp![0].to).toBe(1.5);
    expect(warpedTo(m, 0.5, 2)).toBe(1.5);
  });
});

describe('the stretch builders rescale the map', () => {
  it('stretchClip scales `to` with the offset, and a snap back to 1 unscales it', () => {
    const c = clip({ stretch: 1, warp: [marker(1, 1.5)] });
    const s = stretchClip(c, 'r', c.start + 8, 1, 2); // ×2: both segments stay in range
    expect(s.stretch).toBe(2);
    expect(s.warp![0].to).toBeCloseTo(3, 9);
    const back = stretchClip(s, 'r', c.start + 4, 1, 2); // to plain speed (exact snap)
    expect(back.stretch).toBeUndefined();
    expect(back.warp![0].to).toBeCloseTo(1.5, 9);
  });

  it('a factor a marker segment cannot take stops the drag at the segment bound', () => {
    // marker (1, 1.5): the head slope is 1.5, so ×3 would stretch that segment ×4.5
    const c = clip({ start: 8, len: 4, warp: [marker(1, 1.5)] });
    const s = stretchClip(c, 'r', c.start + 12, 1, 2); // wanted ×3
    expect(s.stretch).toBeCloseTo(4 / 1.5, 6); // stops at the head segment's ×4
    expect(s.warp![0].to).toBeCloseTo(4, 6);
    // a markerless clip still reaches the ×4 baseline cap
    const plainClip = clip({ start: 8, len: 4 });
    expect(stretchClip(plainClip, 'r', plainClip.start + 16, 1, 2).stretch).toBe(4);
  });

  it('fitClip refuses factors the marker segments cannot take', () => {
    const c = clip({ warp: [marker(1, 1.5)] });
    expect(fitClip(c, 12, 2)).toBeNull(); // ×3 → the head segment would stretch ×4.5
    const f = fitClip(c, 8, 2)!; // ×2 keeps every segment inside 0.25..4
    expect(f.stretch).toBe(2);
    expect(f.warp![0].to).toBe(3);
  });

  it('the pin drag is bounded by both adjacent segments, not just the neighbours', () => {
    // the review's case: marker (1, 1) in a 2 s source at stretch 1 — the tail segment
    // bottoms out at ×0.25, so the pin stops at 1.75, not at the neighbour's 1.995
    const c = clip({ warp: [marker(1, 1)] });
    expect(warpDragBounds(c, 0, 2)).toEqual([0.25, 1.75]);
    // 1.4 is inside: both segments (1 and 0.6) stay within 0.25..4
    const [lo, hi] = warpDragBounds(c, 0, 2);
    expect(1.4).toBeGreaterThanOrEqual(lo);
    expect(1.4).toBeLessThanOrEqual(hi);
    // with a neighbour on both sides only the segments between them bound the drag
    const two = clip({ stretch: 2, warp: [marker(1, 3), marker(2, 5)] });
    // left segment slope 2 → to ∈ [3.25, 7]; the implicit end (4, 8) allows [0, 7.5]; spacing 3.005..7.995
    expect(warpDragBounds(two, 1, 4)).toEqual([3.25, 7]);
  });
});

describe('putClips through the mock, like the engine', () => {
  const withClip = async () => {
    const be = new MockBackend();
    await be.importAudio({ paths: ['C:\\loops\\take.wav'], track: 0, beat: 0 });
    const p = (await be.getState()).project;
    return { be, p, c: p.clips.find(x => x.kind === 'audio')! };
  };

  it('stores the map sorted, emptied maps stripped', async () => {
    const { be, c } = await withClip();
    await be.applyOps([{ op: 'putClips', clips: [{ ...c, warp: [marker(1.5, 4), marker(0.5, 1)] }] }], { label: 'warp' });
    const got = (await be.getState()).project.clips.find(x => x.id === c.id)!;
    expect(got.warp!.map(w => w.at)).toEqual([0.5, 1.5]);
    await be.applyOps([{ op: 'putClips', clips: [{ ...got, warp: [] }] }], { label: 'unwarp' });
    expect('warp' in (await be.getState()).project.clips.find(x => x.id === c.id)!).toBe(false);
  });

  it("a rejected op throws the engine's message and changes nothing", async () => {
    const { be, p, c } = await withClip();
    const dur = p.sources.find(s => s.id === c.ref)!.dur;
    await be.applyOps([{ op: 'putClips', clips: [{ ...c, warp: [marker(1, 1)] }] }], { label: 'warp' });
    const put = (warp: WarpPoint[]) => async () => {
      const now = (await be.getState()).project.clips.find(x => x.id === c.id)!;
      await be.applyOps([{ op: 'putClips', clips: [{ ...now, warp }] }], { label: 'warp' });
    };
    await expect(put([marker(dur + 1, 1)])).rejects.toThrow(OpError);
    await expect(put([{ at: NaN, to: 1 }])).rejects.toThrow(/must be finite and not negative/);
    const after = (await be.getState()).project.clips.find(x => x.id === c.id)!;
    expect(after.warp).toEqual([marker(1, 1)]);
    // validation runs before anything moves: the sorted map survived both rejects
    expect(after.start).toBe(c.start);
  });

  it('applyAll probes the op on a clone (a validation helper, not a store write)', async () => {
    const { p, c } = await withClip();
    const clone = structuredClone(p);
    expect(() => applyAll(clone, [{ op: 'putClips', clips: [{ ...c, warp: [marker(durOf(p, c.ref) + 1, 1)] }] }])).toThrow();
    expect(clone.clips.find(x => x.id === c.id)!.warp).toBeUndefined();
    function durOf(pp: typeof p, ref: number) { return pp.sources.find(s => s.id === ref)!.dur; }
  });
});
