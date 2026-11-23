import { beforeAll, describe, expect, it, vi } from 'vitest';
import { MockBackend } from './mock';
import { libPath } from '../lib/factory';
import type { Id, Project } from './types';

beforeAll(() => { vi.stubGlobal('window', {}); });

/** A sung-take setup on the mock: the take clip routed to insert 1 (a same-kind comp already
 *  there, which the chain replaces) and a second clip as the beat's key source. */
async function vocalProject(be: MockBackend): Promise<{ clip: Id; beat: Id; ref0: Id }> {
  await be.projectNew();
  const take = await be.importAudio({ paths: [libPath('triangle.flac')], track: 0, beat: 0 });
  const beatImp = await be.importAudio({ paths: [libPath('kick-808-short.flac')], track: 1, beat: 4 });
  const p = (await be.getState()).project;
  const clip = p.clips.find(c => c.kind === 'audio' && c.ref === take.sourceIds[0])!.id;
  const ref0 = take.sourceIds[0];
  await be.applyOps([{ op: 'putTrack', track: { ...p.tracks[0], insert: 1 } }]);
  await be.applyOps([{ op: 'putFx', insertId: 1, fx: { id: 9002, type: 'comp', bypass: false, preset: null, params: { thr: -20 } } }]);
  return { clip, beat: p.clips.find(c => c.kind === 'audio' && c.ref === beatImp.sourceIds[0])!.id, ref0 };
}

const audio = (p: Project, clip: Id) => p.clips.find(c => c.id === clip) as { ref: Id; kind: 'audio' };

describe('the mock answers autoVocal as one journey, like the engine action', () => {
  it('bakes the rider, replaces the same-kind comp, keeps the reply shape, two undo steps', async () => {
    const be = new MockBackend();
    const { clip, beat, ref0 } = await vocalProject(be);
    const plan = await be.autoVocalPlan({ clipId: clip, beatClipId: beat, bakeRider: true });
    expect(plan.riderBake).not.toBeNull();

    const stages: string[] = [];
    const rev = (await be.getState()).revision;
    const r = await be.autoVocal({ clipId: clip, beatClipId: beat, label: 'Auto wokal' }, s => stages.push(s));
    expect(stages).toEqual(['plan', 'bake', 'chain']);
    expect(r.clipId).toBe(clip);
    expect(r.insertId).toBe(1);
    expect(r.riderBaked).toEqual({ sourceId: expect.any(Number), clipId: clip });
    expect(r.fxIds).toHaveLength(plan.chain.length);
    expect(r.revision).toBe(rev + 2);

    const p = (await be.getState()).project;
    // the bake: the clip plays a " (rider)" source now, the original stays in the project
    const playing = audio(p, clip).ref;
    expect(playing).not.toBe(ref0);
    expect(p.sources.find(s => s.id === playing)!.name.endsWith(' (rider)')).toBe(true);
    expect(p.sources.some(s => s.id === ref0)).toBe(true);
    // the chain: the comp was replaced, the stage ids are the reply's, the limiter is last
    const fx = p.inserts.find(m => m.id === 1)!.fx;
    expect(fx.map(f => f.id)).toEqual(r.fxIds);
    expect(fx.some(f => f.id === 9002)).toBe(false);
    expect(fx.at(-1)!.type).toBe('limit');
    // the chain step carries the caller's undo label; one undo takes it off, restoring the comp
    expect((await be.getState()).undoLabel).toBe('Auto wokal');
    await be.undo();
    expect((await be.getState()).project.inserts.find(m => m.id === 1)!.fx.map(f => f.id)).toEqual([9002]);
  });

  it('does not bake twice: the second run skips the bake stage and adds only the chain step', async () => {
    const backend = new MockBackend();
    const { clip, beat } = await vocalProject(backend);
    await backend.autoVocal({ clipId: clip, beatClipId: beat });
    const rev = (await backend.getState()).revision;
    const stages: string[] = [];
    const r2 = await backend.autoVocal({ clipId: clip, beatClipId: beat }, s => stages.push(s));
    expect(stages).toEqual(['plan', 'chain']);
    expect(r2.riderBaked).toBeNull();
    expect(r2.revision).toBe(rev + 1);
    expect((await backend.getState()).project.sources.filter(s => s.name.endsWith(' (rider)'))).toHaveLength(1);
  });

  it('refuses the master insert like the engine, before any analysis', async () => {
    const backend = new MockBackend();
    await backend.projectNew();
    const take = await backend.importAudio({ paths: [libPath('triangle.flac')], track: 0, beat: 0 });
    const clip = (await backend.getState()).project.clips.find(c => c.kind === 'audio' && c.ref === take.sourceIds[0])!.id;
    const stages: string[] = [];
    await expect(backend.autoVocal({ clipId: clip }, s => stages.push(s))).rejects.toThrow(/master insert/);
    expect(stages, 'the refusal comes before the first stage').toEqual([]);
  });
});
