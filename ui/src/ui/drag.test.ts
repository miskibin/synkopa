import { beforeEach, describe, expect, it, vi } from 'vitest';
import { dblCheck, dragCursor, fineMul, resetDblCheck, track } from './drag';

// a minimal DOM: the root element's classes and custom properties, and window listeners
const cls = new Set<string>(), props = new Map<string, string>();
const listeners = new Map<string, (e: unknown) => void>();
// every listener ever added (dblCheck's capture listeners stay for the life of the page)
const all: { k: string; f: (e: unknown) => void }[] = [];
vi.stubGlobal('document', {
  documentElement: {
    classList: { add: (c: string) => cls.add(c), remove: (c: string) => cls.delete(c) },
    style: { setProperty: (k: string, v: string) => props.set(k, v), removeProperty: (k: string) => props.delete(k) },
  },
});
vi.stubGlobal('window', {
  addEventListener: (k: string, f: (e: unknown) => void) => { listeners.set(k, f); all.push({ k, f }); },
  removeEventListener: (k: string) => listeners.delete(k),
});
beforeEach(() => { cls.clear(); props.clear(); listeners.clear(); });

describe('drag cursor', () => {
  it('forces the drag cursor on the whole page, and releases it', () => {
    dragCursor('grabbing');
    expect(cls.has('dragcur')).toBe(true);
    expect(props.get('--drag-cursor')).toBe('grabbing');
    dragCursor(null);
    expect(cls.has('dragcur')).toBe(false);
    expect(props.has('--drag-cursor')).toBe(false);
  });
  it('shows a closed hand for the whole of a grab drag, released on pointerup', () => {
    const up = vi.fn();
    track(() => {}, up, 'grabbing');
    expect(props.get('--drag-cursor')).toBe('grabbing');
    listeners.get('pointerup')!({});
    expect(cls.has('dragcur')).toBe(false);
    expect(up).toHaveBeenCalledOnce();
  });
  it('releases it when the pointer is cancelled', () => {
    track(() => {}, null, 'grabbing');
    listeners.get('pointercancel')!({});
    expect(cls.has('dragcur')).toBe(false);
  });
  it('leaves the cursor alone for a drag that names none', () => {
    track(() => {});
    expect(cls.has('dragcur')).toBe(false);
  });
});

describe('double click (dblCheck)', () => {
  // fire the capture listeners dblCheck installs on the window
  const fire = (type: string, e: object) => all.filter(l => l.k === type).forEach(l => l.f(e));
  const down = (x: number, y: number) => fire('pointerdown', { clientX: x, clientY: y });
  let now = 0;
  beforeEach(() => { now = 1000; vi.spyOn(performance, 'now').mockImplementation(() => now); dblCheck('install the listeners'); resetDblCheck(); });

  it('is two presses on the same key in the same place within 320 ms', () => {
    down(10, 10); expect(dblCheck('a')).toBe(false);
    now += 200; down(11, 12); expect(dblCheck('a')).toBe(true);
    // a third press starts over
    now += 100; down(11, 12); expect(dblCheck('a')).toBe(false);
  });
  it('is not a slow second press, nor a press on another key', () => {
    down(10, 10); dblCheck('a');
    now += 400; down(10, 10); expect(dblCheck('a')).toBe(false);
    now += 100; down(10, 10); expect(dblCheck('b')).toBe(false);
  });
  it('is not a grab right after a drag: the first press moved', () => {
    down(10, 10); expect(dblCheck('knob')).toBe(false);
    fire('pointermove', { clientX: 10, clientY: 40, buttons: 1 });
    now += 150; down(10, 40); expect(dblCheck('knob')).toBe(false);
  });
  it('is not two presses apart from each other (two controls sharing a key)', () => {
    down(10, 10); dblCheck('nfGain');
    now += 150; down(90, 10); expect(dblCheck('nfGain')).toBe(false);
  });
  it('ignores hover moves between the presses (no button held)', () => {
    down(10, 10); dblCheck('a');
    fire('pointermove', { clientX: 12, clientY: 11, buttons: 0 });
    now += 150; down(12, 11); expect(dblCheck('a')).toBe(true);
  });
  it('fine adjust is Ctrl (FL) or Shift', () => {
    expect(fineMul({ shiftKey: false, ctrlKey: false, metaKey: false })).toBe(1);
    expect(fineMul({ shiftKey: false, ctrlKey: true, metaKey: false })).toBe(0.2);
    expect(fineMul({ shiftKey: true, ctrlKey: false, metaKey: false })).toBe(0.2);
  });
});
