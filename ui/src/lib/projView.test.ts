import { describe, expect, it } from 'vitest';
import type { Project } from '../api/types';
import { parseViews, PROJ_VIEWS_MAX, rememberView, viewFor, type ProjViews } from './projView';

const project = (pats: number[], chans: number[]) => ({ patterns: pats.map(id => ({ id })), channels: chans.map(id => ({ id })) }) as unknown as Project;
const P = project([10, 11, 12], [20, 21]);

describe('reopening a project where you left it', () => {
  it('restores the pattern, channel and start marker saved for that path', () => {
    const views = rememberView([], 'C:\\Songs\\Beat.synkopa', { pat: 12, selCh: 21, start: 128 });
    expect(viewFor(views, 'C:\\Songs\\Beat.synkopa', P, 512)).toEqual({ pat: 12, selCh: 21, start: 128 });
    // the same file named with other slashes or case
    expect(viewFor(views, 'c:/songs/beat.synkopa', P, 512)).toEqual({ pat: 12, selCh: 21, start: 128 });
  });
  it('opens on the first pattern and bar 1 for a project never saved here, or with no path', () => {
    const views = rememberView([], 'a.synkopa', { pat: 12, selCh: 21, start: 8 });
    expect(viewFor(views, 'other.synkopa', P, 512)).toEqual({ pat: 10, selCh: 20, start: 0 });
    expect(viewFor(views, null, P, 512)).toEqual({ pat: 10, selCh: 20, start: 0 });
    expect(viewFor([], 'a.synkopa', project([], []), 512)).toEqual({ pat: null, selCh: null, start: 0 });
  });
  it('falls back for a pattern or channel that is gone and a marker outside the song', () => {
    const views = rememberView([], 'a.synkopa', { pat: 99, selCh: 21, start: 9000 });
    expect(viewFor(views, 'a.synkopa', P, 512)).toEqual({ pat: 10, selCh: 21, start: 0 });
  });
  it('keeps one entry per project, the newest last, and forgets the oldest past the limit', () => {
    let views: ProjViews = [];
    for (let i = 0; i < PROJ_VIEWS_MAX + 5; i++) views = rememberView(views, 'p' + i, { pat: i, selCh: null, start: 0 });
    views = rememberView(views, 'P10', { pat: 777, selCh: null, start: 0 });
    expect(views).toHaveLength(PROJ_VIEWS_MAX);
    expect(views.at(-1)).toEqual(['p10', { pat: 777, selCh: null, start: 0 }]);
    expect(views.filter(([k]) => k === 'p10')).toHaveLength(1);
    expect(views.some(([k]) => k === 'p0')).toBe(false);
  });
  it('reads back what it stores and drops malformed data', () => {
    const views = rememberView([], 'a.synkopa', { pat: 12, selCh: null, start: 4 });
    expect(parseViews(JSON.stringify(views))).toEqual(views);
    expect(parseViews(null)).toEqual([]);
    expect(parseViews('not json')).toEqual([]);
    expect(parseViews('{"a":1}')).toEqual([]);
    expect(parseViews('[["a",{"pat":"x","start":"y"}],[1,2],"z"]')).toEqual([['a', { pat: null, selCh: null, start: 0 }]]);
  });
});
