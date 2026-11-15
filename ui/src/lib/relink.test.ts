import { describe, expect, it } from 'vitest';
import { candidate, fileName, folderOf, rank, unambiguous } from './relink';

// the same cases as crates/engine/src/relink.rs
describe('relink ranking', () => {
  const old = 'C:\\Old\\Drums\\kick.wav';
  it('matches the name ignoring case, and a converted file by its stem only', () => {
    expect(candidate(old, '/x/a/Drums/KICK.wav')).toEqual({ path: '/x/a/Drums/KICK.wav', exactName: true, sharedFolders: 1 });
    expect(candidate(old, '/x/kick.flac')).toMatchObject({ exactName: false, sharedFolders: 0 });
    expect(candidate(old, '/x/snare.wav')).toBeNull();
  });
  it('ranks by name, then shared folders, then path; takes only a single best exact match', () => {
    const r = rank(['/b/Other/kick.wav', '/c/kick.flac', '/a/Drums/kick.wav'].map(p => candidate(old, p)!));
    expect(r.map(c => c.path)).toEqual(['/a/Drums/kick.wav', '/b/Other/kick.wav', '/c/kick.flac']);
    expect(unambiguous(r)?.path).toBe('/a/Drums/kick.wav');
    expect(unambiguous(rank(['/x/kick.wav', '/y/kick.wav'].map(p => candidate(old, p)!)))).toBeNull();
    expect(unambiguous([candidate(old, '/x/kick.mp3')!])).toBeNull();
    expect(unambiguous([])).toBeNull();
  });
  it('splits stored paths on either separator', () => {
    expect(fileName('C:\\a\\b.wav')).toBe('b.wav');
    expect(folderOf('C:\\a\\b.wav')).toBe('C:\\a');
    expect(folderOf('/a/b.wav')).toBe('/a');
  });
});
