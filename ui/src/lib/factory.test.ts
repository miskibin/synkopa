import { describe, expect, it } from 'vitest';
import { browserSources, emptyProject, libPath, STARTER_KIT } from './factory';

describe('browserSources', () => {
  const src = (id: number, path: string) => ({ id, name: 's' + id, color: '#fff', path, dur: 1, sr: 48000, ch: 1, peakDb: 0 });

  it('leaves out library samples only the Channel Rack plays', () => {
    const p = emptyProject();
    const rack = new Set(p.sources.map(s => s.id));
    expect(STARTER_KIT.length).toBeGreaterThan(0);
    expect(browserSources(p).filter(s => rack.has(s.id))).toEqual([]);
  });

  it('lists imports, and a library sample once it has a clip', () => {
    const p = emptyProject();
    const lib = p.sources.find(s => s.path.startsWith('lib:'));
    p.sources.push(src(9001, 'C:/music/vox.wav'));
    expect(browserSources(p).map(s => s.id)).toContain(9001);
    if (lib) {
      p.clips.push({ id: 9002, kind: 'audio', ref: lib.id, track: 0, start: 0, len: 4 } as never);
      expect(browserSources(p).map(s => s.id)).toContain(lib.id);
    }
    // a library sample nothing in the rack plays (dropped here from the Library) is the user's too
    p.sources.push(src(9003, libPath('x.flac')));
    expect(browserSources(p).map(s => s.id)).toContain(9003);
  });
});
