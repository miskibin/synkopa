// Convert to MIDI in the mock: the same contract as the engine's `audio_to_midi`
// (app/src/audio_midi_actions_tests.rs) — a preview that changes nothing, a convert that writes channel,
// pattern and Playlist clip in one undo step, and the same refusals.
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { MockBackend } from './mock';
import { libPath } from '../lib/factory';

beforeAll(() => { vi.stubGlobal('window', {}); });

async function setup() {
  const be = new MockBackend();
  await be.projectNew();
  const imp = await be.importAudio({ paths: [libPath('triangle.flac')], track: 0, beat: 4 });
  return { be, clipId: imp.clipIds[0] };
}

describe('audioToMidi (mock)', () => {
  it('previews without changing anything, then converts in one undo step', async () => {
    const { be, clipId } = await setup();
    const rev0 = (await be.getState()).revision;
    const pv = await be.audioToMidi({ clipId, preview: true });
    expect(pv.count).toBeGreaterThan(0);
    expect(pv.notes).toHaveLength(pv.count);
    expect(pv.patternId).toBeUndefined();
    expect((await be.getState()).revision).toBe(rev0);
    // every note on the song inside the clip, and keys in the take's range
    const st = await be.getState();
    const clip = st.project.clips.find(c => c.id === clipId)!;
    for (const n of pv.notes) {
      const beat = pv.start + n.step / 4;
      expect(beat).toBeGreaterThanOrEqual(clip.start - 1e-6);
      expect(beat + n.len / 4).toBeLessThanOrEqual(clip.start + clip.len + 1e-6);
    }
    const r = await be.audioToMidi({ clipId, quantize: 1 });
    expect(r.newChannel).toBe(true);
    expect(r.clipId).toBe(clipId);
    const s2 = await be.getState();
    expect(s2.revision).toBe(rev0 + 1);
    const pat = s2.project.patterns.find(p => p.id === r.patternId)!;
    expect(pat.name).toBe('Melody – Triangle');
    expect(pat.notes.every(n => n.ch === r.channelId && Number.isInteger(n.step))).toBe(true);
    const pc = s2.project.clips.find(c => c.id === r.patternClipId)!;
    expect(pc.kind).toBe('pat');
    expect(pc.track).not.toBe(clip.track);
    await be.undo();
    expect((await be.getState()).project.patterns.some(p => p.id === r.patternId)).toBe(false);
  });
  it('leaves out keys outside the range and refuses what it cannot do', async () => {
    const { be, clipId } = await setup();
    const all = await be.audioToMidi({ clipId, preview: true });
    const top = all.high!;
    const lower = await be.audioToMidi({ clipId, preview: true, high: top - 1 });
    expect(lower.notes.every(n => n.p < top)).toBe(true);
    await expect(be.audioToMidi({ clipId, low: 80, high: 60 })).rejects.toThrow('above');
    await expect(be.audioToMidi({ clipId, low: 300 })).rejects.toThrow('0–127');
    await expect(be.audioToMidi({ clipId, channel: 999_999 })).rejects.toThrow('no channel');
    const none = await be.audioToMidi({ clipId, preview: true, low: 127 });
    expect(none.count).toBe(0);
    await expect(be.audioToMidi({ clipId, low: 127 })).rejects.toThrow('no notes');
  });
});
