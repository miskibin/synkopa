import { describe, expect, it } from 'vitest';
import { isDefaultInsertName, planRoute, rangeSelect, restoreArrange, stripLabel, takeArrange, toggleSelect, type Arrange } from './mixer';

describe('stripLabel', () => {
  it('shows the Master by name, without a number', () => {
    expect(stripLabel('Master', 0)).toEqual({ num: '', name: 'Master', full: 'Master' });
  });
  it('shows a default "Insert N" as just its place', () => {
    expect(stripLabel('Insert 7', 7)).toEqual({ num: '7', name: '', full: '7 · Insert 7' });
    // after inserts were removed the default name and the place can differ: the place wins on the strip
    expect(stripLabel('Insert 9', 4)).toMatchObject({ num: '4', name: '' });
    expect(stripLabel('', 3)).toEqual({ num: '3', name: '', full: '3 · Insert 3' });
  });
  it('keeps a name the user gave, next to the number', () => {
    expect(stripLabel('Drums', 1)).toEqual({ num: '1', name: 'Drums', full: '1 · Drums' });
    expect(stripLabel('Insert vocals', 2)).toMatchObject({ num: '2', name: 'Insert vocals' });
  });
  it('knows the default names', () => {
    expect(isDefaultInsertName('Insert 12')).toBe(true);
    expect(isDefaultInsertName('insert 3')).toBe(true);
    expect(isDefaultInsertName('Insert')).toBe(false);
    expect(isDefaultInsertName('Bass')).toBe(false);
  });
});

describe('planRoute (Ctrl+L / Shift+Ctrl+L)', () => {
  const ins = [{ id: 0 }, { id: 10 }, { id: 11 }, { id: 12 }];
  const ch = (id: number, name: string) => ({ id, name });
  const ids = (start: number) => { let n = start; return () => n++; };

  it('routes every selected channel to the insert', () => {
    expect(planRoute([ch(1, 'Kick'), ch(2, 'Snare')], ins, 11, false, ids(100))).toEqual({ assign: [[1, 11], [2, 11]], create: [] });
  });
  it('can route to the Master', () => {
    expect(planRoute([ch(1, 'Kick')], ins, 0, false, ids(100))).toEqual({ assign: [[1, 0]], create: [] });
  });
  it('spreads over consecutive inserts', () => {
    expect(planRoute([ch(1, 'Kick'), ch(2, 'Snare')], ins, 10, true, ids(100))).toEqual({ assign: [[1, 10], [2, 11]], create: [] });
  });
  it('creates the missing inserts, named after their channel', () => {
    const p = planRoute([ch(1, 'Kick'), ch(2, 'Snare'), ch(3, 'Hat')], ins, 12, true, ids(100))!;
    expect(p.assign).toEqual([[1, 12], [2, 100], [3, 101]]);
    expect(p.create).toEqual([{ id: 100, name: 'Snare' }, { id: 101, name: 'Hat' }]);
  });
  it('does nothing without a selection, an insert, or when spreading from the Master', () => {
    expect(planRoute([], ins, 10, false, ids(100))).toBeNull();
    expect(planRoute([ch(1, 'Kick')], ins, 99, false, ids(100))).toBeNull();
    expect(planRoute([ch(1, 'Kick')], ins, 0, true, ids(100))).toBeNull();
  });
});

describe('channel multi-selection', () => {
  const order = [1, 2, 3, 4, 5];
  it('Shift+click selects the range from the anchor, either way', () => {
    expect(rangeSelect(order, 2, 4)).toEqual([2, 3, 4]);
    expect(rangeSelect(order, 4, 2)).toEqual([2, 3, 4]);
    expect(rangeSelect(order, 3, 3)).toEqual([3]);
  });
  it('without an anchor a range is the clicked channel', () => {
    expect(rangeSelect(order, null, 4)).toEqual([4]);
    expect(rangeSelect(order, 99, 4)).toEqual([4]);
    expect(rangeSelect(order, 2, 99)).toEqual([]);
  });
  it('Shift+Ctrl+click adds and removes one channel', () => {
    expect(toggleSelect([], 2, 4)).toEqual([2, 4]);
    expect(toggleSelect([2, 4], 2, 4)).toEqual([2]);
    expect(toggleSelect([2, 4], 2, 2)).toEqual([4]);
    expect(toggleSelect([], null, 4)).toEqual([4]);
  });
});

describe('Mix view snapshot of the arrange layout', () => {
  const arrange = (): Arrange => ({
    rackWin: { x: 10, y: 60, w: 700, h: 400, z: 3 }, rollWin: null, rackDock: false, rollDock: true, rackDockW: 0, rollDockH: 320,
    rollMax: false, rackFloat: { x: 10, y: 60, w: 700, h: 400 }, rollFloat: null, plugsHidden: false, mixH: 260, mixMin: true, pickOpen: true, pickW: 240,
  });
  it('gives back exactly what was taken', () => {
    const u = arrange();
    const snap = takeArrange(u);
    expect(restoreArrange(snap)).toEqual(arrange());
  });
  it('is detached from the live state (moving a window later does not change it)', () => {
    const u = arrange();
    const snap = takeArrange(u);
    u.rackWin!.x = 500; u.mixH = 400; u.rollDock = false;
    const back = restoreArrange(snap);
    expect(back.rackWin).toEqual({ x: 10, y: 60, w: 700, h: 400, z: 3 });
    expect(back).toMatchObject({ mixH: 260, rollDock: true, mixMin: true });
    // and the restore patch is a copy too: the snapshot can be restored again
    back.rackWin!.y = 0;
    expect(restoreArrange(snap).rackWin!.y).toBe(60);
  });
  it('holds only layout fields', () => {
    const snap = takeArrange({ ...arrange(), tool: 'paint', mixView: true } as Arrange);
    expect(Object.keys(snap).sort()).toEqual(Object.keys(arrange()).sort());
  });
});
