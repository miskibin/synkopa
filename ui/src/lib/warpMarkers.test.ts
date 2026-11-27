// Warp markers on audio clips: the map math, the editors that keep it consistent (stretch,
// fit), and the op path through the mock (sorted storage, stripped when emptied, rejections).
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { MockBackend } from '../api/mock';
import { applyAll, OpError } from './applyOps';
import { type Clip, type WarpPoint } from '../api/types';
import { fitClip, markersAtTransients, moveWarp, pinWarp, removeWarp, stretchClip, warpActive, warpedFrom, warpedTo } from './clipOps';

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

  it('pins markers at transients at their mapped places, dropping the too-close ones', () => {
    const c = clip({ warp: [marker(0.5, 0.5)] });
    const m = markersAtTransients(c, [0.1, 0.5000001, 1.2, 1.2000001], 2)!;
    expect(m.warp!.map(w => w.at)).toEqual([0.1, 0.5, 1.2]);
    // the new pin lands where the old map put 1.2 s of source: pinning never jumps the audio
    expect(m.warp![2].to).toBe(warpedTo(c, 1.2, 2));
  });
});

describe('the stretch builders rescale the map', () => {
  it('stretchClip scales `to` with the offset, and a snap back to 1 unscales it', () => {
    const c = clip({ stretch: 2, warp: [marker(1, 3)] });
    const s = stretchClip(c, 'r', c.start + 8, 1); // to the ×4 cap
    expect(s.stretch).toBe(4);
    expect(s.warp![0].to).toBeCloseTo(6, 9);
    const back = stretchClip(s, 'r', c.start + 2, 1); // to plain speed (exact snap)
    expect(back.stretch).toBeUndefined();
    expect(back.warp![0].to).toBeCloseTo(1.5, 9);
  });

  it('fitClip scales `to` with the stretch it sets', () => {
    const f = fitClip(clip({ stretch: 2, warp: [marker(1, 3)] }), 8)!;
    expect(f.stretch).toBe(4);
    expect(f.warp![0].to).toBe(6);
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
