import { describe, expect, it } from 'vitest';
import { addRecent, parseRecent, RECENT_LIMIT } from './recentSamples';

const file = (i: number) => ({ path: 'lib:' + i, name: 'Sample ' + i, group: 'Kicks' });
describe('recent samples', () => {
  it('keeps 30 newest unique paths and moves a reused sample to the top', () => {
    let list = Array.from({ length: 40 }, (_, i) => file(i)).reduce((acc, f) => addRecent(acc, f), [] as ReturnType<typeof addRecent>);
    expect(list).toHaveLength(RECENT_LIMIT);
    expect(list[0]).toEqual(file(39));
    list = addRecent(list, { ...file(15), name: 'Renamed' });
    expect(list[0].name).toBe('Renamed');
    expect(list.filter(f => f.path === file(15).path)).toHaveLength(1);
    expect(list).toHaveLength(30);
  });
  it('ignores malformed data, duplicate paths and invalid roots', () => {
    expect(parseRecent('oops')).toEqual([]);
    expect(parseRecent(JSON.stringify([null, {}, { ...file(1), root: 500 }, file(1), { ...file(2), root: 60, pitched: true }]))).toEqual([file(1), { ...file(2), root: 60, pitched: true }]);
  });
});
