import { describe, expect, it } from 'vitest';
import { FACTORY_LISTED_FILES, libPath } from '../lib/factory';
import { channelIcon, guessIcon, laneAutoIcons, patternIcon, sampleIcon } from './instIcons';

describe('guessIcon', () => {
  it('names an 808 drum as drums and an 808 bass as bass', () => {
    for (const n of ['808 Kick Short', '808 Clap', '808 Closed Hat', '808 Snare']) expect(guessIcon(n)).toBe('drums');
    expect(guessIcon('808 Bass')).toBe('bass');
    expect(guessIcon('Sub 808')).toBe('bass');
  });
  it('recognises the common names', () => {
    expect(guessIcon('looperman-brazilian-guitar-sacos')).toBe('guitar');
    expect(guessIcon('Lead Vox')).toBe('vocal');
    expect(guessIcon('Pad Loop')).toBe('synth');
    expect(guessIcon('Untitled')).toBeNull();
  });
  it('does not read "rap" or "chór" inside other words', () => {
    for (const n of ['Kick Trap Knock', 'Snare Trap', 'Clap Trap Crisp', 'Hat Trap Tick']) expect(guessIcon(n)).toBe('drums');
    expect(guessIcon('Chord Stab')).toBe('synth');
    expect(guessIcon('Rap verse')).toBe('vocal');
    expect(guessIcon('Chór')).toBe('vocal');
  });
});

const lib = (file: string) => { const f = FACTORY_LISTED_FILES.find(x => x.file === file)!; return { name: f.name, path: libPath(f.file) }; };

describe('sample and channel icons', () => {
  it('every drum and 808 file of the standard library gets its group icon, whatever its name', () => {
    for (const f of FACTORY_LISTED_FILES) {
      const want = ({ Kicks: 'drums', Snares: 'drums', Claps: 'drums', Hats: 'drums', Cymbals: 'drums', Toms: 'drums', Percussion: 'drums', 808: 'bass', FX: 'fx' } as Record<string, string>)[f.group];
      if (want) expect(sampleIcon({ name: f.name, path: libPath(f.file) }), f.file).toBe(want);
    }
  });
  it('a sample channel takes its library sample group before its name', () => {
    const s = lib('sleigh-bells.flac'), src = (id: number) => (id === 7 ? s : undefined);
    expect(channelIcon({ name: s.name, kind: 'sample', source: 7 }, src)).toBe('drums');
    // a user's own sample: the channel name, then the file name, then the plain sample icon
    const own = (id: number) => (id === 8 ? { name: 'take 3', path: 'C:/rec/take 3.wav' } : undefined);
    expect(channelIcon({ name: 'Lead Vox', kind: 'sample', source: 8 }, own)).toBe('vocal');
    expect(channelIcon({ name: 'take 3', kind: 'sample', source: 8 }, own)).toBe('sample');
  });
});

describe('split-by-channel patterns', () => {
  const chans = new Map([[1, { name: 'Kick', kind: 'sample' as const }], [2, { name: '808 Bass', kind: 'sample' as const }], [3, { name: 'Pluck', kind: 'synth' as const }]]);
  it('a one-channel pattern shows its channel, not the parent pattern name it repeats', () => {
    expect(patternIcon({ name: 'Beat – 808 Bass', notes: [{ ch: 2 }, { ch: 2 }] }, chans)).toBe('bass');
    expect(patternIcon({ name: 'Beat – Pluck', notes: [{ ch: 3 }] }, chans)).toBe('synth');
  });
  it('a pattern of several channels keeps its name first, then the most used channel', () => {
    expect(patternIcon({ name: 'Beat', notes: [{ ch: 2 }, { ch: 3 }, { ch: 3 }] }, chans)).toBe('drums');
    expect(patternIcon({ name: 'Pattern 2', notes: [{ ch: 2 }, { ch: 3 }, { ch: 3 }] }, chans)).toBe('synth');
  });
  it('lanes without an icon of their own show their clips\' icon', () => {
    const p = {
      tracks: [{ name: 'Drums' }, { name: 'Track 2' }, { name: 'Track 3', icon: 'fx' }, { name: 'Track 4' }, { name: 'Track 5' }],
      clips: [{ track: 0, kind: 'pat', ref: 10 }, { track: 1, kind: 'pat', ref: 11 }, { track: 1, kind: 'pat', ref: 11 }, { track: 2, kind: 'pat', ref: 11 }, { track: 3, kind: 'audio', ref: 5 }],
      patterns: [{ id: 10, name: 'Beat – Pluck', notes: [{ ch: 3 }] }, { id: 11, name: 'Beat – 808 Bass', notes: [{ ch: 2 }] }],
      channels: [...chans].map(([id, c]) => ({ id, ...c })),
      sources: [{ id: 5, ...lib('kick-808.flac') }],
    };
    // Drums is named, Track 3 has its own icon, Track 5 is empty
    expect(laneAutoIcons(p)).toEqual([null, 'bass', null, 'drums', null]);
  });
});
