// The root note of an audio clip (the sample editor's keyboard): shared by the sample's clips, checked like the
// engine (ops.rs), played from the keyboard relative to it, and inherited by a sampler made from the sample.
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { MockBackend } from '../api/mock';
import type { Clip } from '../api/types';
import { applyAll, OpError } from './applyOps';
import { putSoundClips, sharedSound } from './clipOps';

const clip = (id: number, ref: number, extra: Partial<Clip> = {}): Clip => ({
  id, track: 0, start: id * 4, len: 2, offset: 0, kind: 'audio', ref, muted: false, gain: 0, fadeIn: 0, fadeOut: 0, pan: 0, norm: false, rev: false, ...extra,
});

beforeAll(() => { vi.stubGlobal('window', {}); });

describe('audio clip root note', () => {
  it('is the sample\'s: setting it reaches every clip of the sample, and a new clip starts with it', () => {
    const clips = [clip(1, 10), clip(2, 10), clip(3, 11)];
    const op = putSoundClips(clips, clips[0], { root: 57 });
    expect(op.op === 'putClips' && op.clips.map(c => [c.id, c.root])).toEqual([[1, 57], [2, 57]]);
    expect(sharedSound([clip(1, 10, { root: 57 })], 10)).toMatchObject({ root: 57 });
  });

  it('must be a MIDI key, like the engine', async () => {
    const be = new MockBackend();
    await be.importAudio({ paths: ['C:\loops\keys.wav'], track: 0, beat: 0 });
    const p = (await be.getState()).project;
    const c = p.clips.find(x => x.kind === 'audio')!;
    const put = (root: number) => applyAll(structuredClone(p), [{ op: 'putClips', clips: [{ ...c, root }] }]);
    expect(() => put(57)).not.toThrow();
    for (const bad of [-1, 128, 60.5]) expect(() => put(bad)).toThrow(OpError);
  });

  it('the keyboard plays the clip transposed from its root and refuses a missing clip', async () => {
    const be = new MockBackend();
    await be.importAudio({ paths: ['C:\loops\keys.wav'], track: 0, beat: 0 });
    const c = (await be.getState()).project.clips.find(x => x.kind === 'audio')!;
    await be.applyOps([{ op: 'putClips', clips: [{ ...c, root: 57 }] }], { label: 'root' });
    const preview = () => (be as unknown as { preview: { dur: number; rate?: number } | null }).preview;
    await be.previewClipKey({ clipId: c.id, key: 69 });
    expect(preview()?.rate).toBeCloseTo(2, 6);
    await be.previewClipKey({ clipId: c.id, key: 57 });
    expect(preview()?.rate).toBeCloseTo(1, 6);
    await expect(be.previewClipKey({ clipId: 99999, key: 60 })).rejects.toThrow();
  });
});
