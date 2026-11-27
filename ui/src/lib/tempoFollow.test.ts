import { describe, expect, it } from 'vitest';
import type { Clip, Project } from '../api/types';
import { tempoClipChanges } from './tempoFollow';

const clip = (followTempo: boolean): Clip => ({ id: 1, track: 0, start: 4, len: 4, offset: 1, kind: 'audio', ref: 9, muted: false, gain: 0, fadeIn: 0, fadeOut: 0, pan: 0, norm: false, rev: false, followTempo });
const project = (c: Clip) => ({ bpm: 80, clips: [c] } as Project);
const span = (c: Clip, bpm: number) => [c.offset, c.offset + c.len].map(b => b * 60 / bpm / (c.stretch ?? 1));

describe('constant tempo edits', () => {
  it.each([false, true])('preserve source region with followTempo=%s', follow => {
    const c = clip(follow);
    const changed = tempoClipChanges(project(c), 100)[0][1];
    const expected = span(c, 80);
    span(changed, 100).forEach((v, i) => expect(v).toBeCloseTo(expected[i]));
    expect(changed.start).toBe(c.start);
    expect(changed.len).toBe(follow ? 4 : 5);
    expect(changed.stretch ?? 1).toBeCloseTo(follow ? 0.8 : 1);
    expect(c.stretch).toBeUndefined();
  });
  it('rejects a tempo outside the stretch range rather than losing part of the recording', () => {
    expect(() => tempoClipChanges(project({ ...clip(true), stretch: 0.25 }), 160)).toThrow('tempo change exceeds stretch range');
  });
});
