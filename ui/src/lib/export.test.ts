import { describe, expect, it } from 'vitest';
import { mockCatalog, sketchProject } from '../api/mockData';
import { exportBytes, exportOverall, exportRemaining, fmtMs, safeFileStem, stemInserts, uniqueFileStems, wavePath } from './export';

const sketch = () => sketchProject(mockCatalog());
const names = (p = sketch()) => stemInserts(p).map(m => m.name);

describe('stem export: which inserts get a file', () => {
  it('lists the inserts with clips routed to them, not the master and not empty inserts', () => {
    expect(names()).toEqual(['Drums', 'Hats', 'Bass']);
  });
  it('leaves out a muted insert and a muted track, like the engine', () => {
    const p = sketch();
    p.inserts.find(m => m.name === 'Hats')!.muted = true;
    p.tracks.find(t => t.insert === p.inserts.find(m => m.name === 'Bass')!.id)!.muted = true;
    expect(names(p)).toEqual(['Drums']);
    p.tracks.forEach(t => { t.muted = true; });
    expect(names(p)).toEqual([]);
  });
  it('counts a channel sent to an insert even without a clip on its track', () => {
    const p = sketch();
    const vocal = p.inserts.find(m => m.name === 'Vocal')!;
    expect(names(p)).not.toContain('Vocal');
    p.channels![0].insert = vocal.id;
    expect(names(p)).toContain('Vocal');
    p.channels![0].mute = true;
    expect(names(p)).not.toContain('Vocal');
  });
});

// the same cases as the Rust tests of `safe_file_stem` / `unique_file_stems` (app/src/actions.rs)
describe('stem export: file names', () => {
  it('makes names safe on every file system', () => {
    expect(safeFileStem('Kick / Snare: 2')).toBe('Kick _ Snare_ 2');
    expect(safeFileStem('  Lead...  ')).toBe('Lead');
    expect(safeFileStem('')).toBe('Insert');
    expect(safeFileStem('???')).toBe('___');
    expect(safeFileStem('con')).toBe('_con');
    expect(safeFileStem('NUL.txt')).toBe('_NUL.txt');
    expect(safeFileStem('COM1')).toBe('_COM1');
    expect(safeFileStem('COM0')).toBe('COM0');
    expect(safeFileStem('Wokal główny')).toBe('Wokal główny');
    expect(safeFileStem('x'.repeat(300))).toHaveLength(100);
  });
  it('makes them unique without regard to case', () => {
    expect(uniqueFileStems(['Bass', 'bass', 'BASS', 'Bass (2)'])).toEqual(['Bass', 'bass (2)', 'BASS (3)', 'Bass (2) (2)']);
    expect(uniqueFileStems(['Master', 'Master'])).toEqual(['Master', 'Master (2)']);
  });
});

describe('export summary and progress', () => {
  it('estimates the file size from length, rate and encoding', () => {
    // 60 s of 44.1 kHz stereo 16-bit: the CD rate, 10.6 MB, plus the 44-byte header
    expect(exportBytes(60, 44100, { format: 'wav', bits: 16 })).toBe(44 + 60 * 44100 * 4);
    expect(exportBytes(60, 48000, { format: 'wav', bits: 24 })).toBe(44 + 60 * 48000 * 6);
    expect(exportBytes(60, 48000, { format: 'wav', bits: 32 })).toBe(44 + 60 * 48000 * 8);
    expect(exportBytes(60, 48000, { format: 'mp3', kbps: 320 })).toBe(2_400_000);
    expect(exportBytes(-1, 48000, { format: 'mp3', kbps: 128 })).toBe(0);
  });
  it('combines finished files and the current stage into one bar that only moves forward', () => {
    const steps = [
      { done: 0, total: 2, stage: 'prepare', frac: 0 }, { done: 0, total: 2, stage: 'render', frac: 0.5 },
      { done: 0, total: 2, stage: 'render', frac: 1 }, { done: 0, total: 2, stage: 'write', frac: 1 },
      { done: 1, total: 2, stage: 'prepare', frac: 0 }, { done: 1, total: 2, stage: 'render', frac: 0.5 },
      { done: 1, total: 2, stage: 'write', frac: 1 }, { done: 2, total: 2, stage: 'done', frac: 1 },
    ];
    const v = steps.map(exportOverall);
    expect(v[0]).toBe(0);
    expect(v.at(-1)).toBe(1);
    expect(v.every((x, i) => i === 0 || x >= v[i - 1])).toBe(true);
    expect(v[5]).toBeCloseTo(0.75, 5);
  });
  it('guesses the time left only once there is something to go on', () => {
    expect(exportRemaining(300, 0.5)).toBeNull();
    expect(exportRemaining(4000, 0.01)).toBeNull();
    expect(exportRemaining(4000, 0.25)).toBe(12000);
    expect(exportRemaining(4000, 1)).toBe(0);
    expect(fmtMs(12000)).toBe('0:12');
    expect(fmtMs(61_200)).toBe('1:02');
  });
  it('draws only the slices that have sound', () => {
    expect(wavePath([], 240, 48)).toBe('');
    expect(wavePath([0, 255, 0], 3, 10).match(/M/g)).toHaveLength(1);
    expect(wavePath([0, 255, 0], 3, 10)).toContain('M1.00 0.00');
  });
});
