// Every surface of the app behind its error boundary (ui/PanelBoundary.tsx), in the whole app:
// a render error in one panel, window, dialog, menu or toolbar part shows the failure there, the
// rest of the app keeps updating and saving, and the surface comes back working ("Try again", or
// by itself once its cause is undone) without leaking or doubling its listeners and frame tasks,
// in the same place.
//
// Where a realistic cause was found, the failure comes from the engine's state (a project item
// with a missing field, read in a binding). Where none was, a translation lookup of that surface
// throws (`bomb`): a stand-in for any render-time bug, in the spirit of Mixer.test.tsx.
import { fireEvent, render, screen } from '@solidjs/testing-library';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { StateDto } from '../api/types';
import type { MockBackend } from '../api/mock';
import { syncEngine } from '../test/engine';
import { nextTask, settle } from '../test/settle';
import { setLang } from '../lib/i18n';

const bomb = vi.hoisted(() => ({ keys: new Set<string>() }));
vi.mock('../lib/i18n', async orig => {
  const real = await orig<typeof import('../lib/i18n')>();
  return { ...real, t: (key: string, vars?: Record<string, string | number>) => { if (bomb.keys.has(key)) throw new Error('render bug: ' + key); return real.t(key as never, vars); } };
});
// the frame tasks alive (canvases, meters, playheads): a rebuilt surface must leave as many as it found
const frames = vi.hoisted(() => ({ live: new Set<unknown>() }));
vi.mock('../ui/frameLoop', async orig => {
  const real = await orig<typeof import('../ui/frameLoop')>();
  return { ...real, addFrameTask: (t: (now: number) => void) => { frames.live.add(t); const off = real.addFrameTask(t); return () => { frames.live.delete(t); off(); }; } };
});

// jsdom has no canvas: an inert 2D context, so the canvas views render (their drawing is not under test)
const ctx: unknown = new Proxy({}, {
  get(_t, k) {
    if (k === 'measureText') return function measure() { return { width: 0 }; };
    return function any() { return { addColorStop() {}, data: new Uint8ClampedArray(4) }; };
  },
  set: () => true,
});
/** The elements size observers watch (a view's canvases follow them): a rebuilt surface must be watched, not the one it replaced. */
const observed = { live: new Set<{ els: Set<Element> }>(), els: () => [...observed.live].flatMap(o => [...o.els]) };
vi.stubGlobal('ResizeObserver', class {
  els = new Set<Element>();
  observe(el: Element) { this.els.add(el); observed.live.add(this); }
  unobserve(el: Element) { this.els.delete(el); }
  disconnect() { this.els.clear(); observed.live.delete(this); }
});
/** Window listeners alive, by type: a rebuilt surface must not leave one behind or add one twice. */
const listening = new Map<string, number>();
beforeAll(() => {
  HTMLCanvasElement.prototype.getContext = (() => ctx) as never;
  (globalThis as Record<string, unknown>).Path2D = class { moveTo() {} lineTo() {} rect() {} arc() {} closePath() {} bezierCurveTo() {} quadraticCurveTo() {} addPath() {} };
  const add = window.addEventListener.bind(window), remove = window.removeEventListener.bind(window);
  const live = new Set<string>(), ids = new WeakMap<object, number>();
  let n = 0;
  const key = (type: string, fn: unknown, o?: boolean | AddEventListenerOptions) => {
    const f = fn as object; if (!ids.has(f)) ids.set(f, ++n);
    return type + ':' + ids.get(f) + ':' + (typeof o === 'boolean' ? o : !!o?.capture);
  };
  const count = () => { listening.clear(); for (const k of live) { const t = k.split(':')[0]; listening.set(t, (listening.get(t) ?? 0) + 1); } };
  window.addEventListener = ((type: string, fn: EventListenerOrEventListenerObject, o?: boolean | AddEventListenerOptions) => { if (fn) live.add(key(type, fn, o)); count(); add(type, fn, o); }) as typeof window.addEventListener;
  window.removeEventListener = ((type: string, fn: EventListenerOrEventListenerObject, o?: boolean | EventListenerOptions) => { if (fn) live.delete(key(type, fn, o as boolean | AddEventListenerOptions)); count(); remove(type, fn, o); }) as typeof window.removeEventListener;
});

let reported: unknown[] = [];
let mock: MockBackend;
let store: typeof import('../state/store');
let edit: typeof import('../state/edit');
let A: typeof import('../state/actions');
let ui: typeof import('../state/ui');
let pat: typeof import('../state/pattern');
let base: StateDto;

beforeEach(async () => {
  reported = []; bomb.keys.clear();
  vi.spyOn(globalThis, 'queueMicrotask').mockImplementation(cb => { Promise.resolve().then(() => { try { cb(); } catch (e) { reported.push(e); } }); });
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  store = await import('../state/store');
  edit = await import('../state/edit');
  A = await import('../state/actions');
  ui = await import('../state/ui');
  pat = await import('../state/pattern');
  if (!store.S.catalog) {
    // a project with every kind of thing a surface shows: an audio clip, a synth channel
    store.setS('catalog', await mock.fxCatalog());
    await mock.importAudio({ paths: ['C:/Music/Vocal take.wav'], track: 0, beat: 0 });
    await syncEngine(mock, store, edit);
    pat.addSynth(null);
  }
  await syncEngine(mock, store, edit);
  base = await mock.getState();
  ui.setU({ mixView: false, pickOpen: true, browserTab: 'patterns', rackWin: null, rollWin: null, plugs: [], editor: null, pitchEd: null, menu: null, settings: false, help: false });
  const p = store.S.project.patterns[0];
  ui.setU('cur', p ? { kind: 'pat', ref: p.id, clipId: null } : null);
  const { App } = await import('../App');
  const { AppBoundary } = await import('../ui/PanelBoundary');
  render(() => <AppBoundary><App /></AppBoundary>);
  settle();
});
afterEach(async () => {
  bomb.keys.clear();
  // close what a test opened (module state outlives the render)
  ui.setU({ settings: false, exportOpen: false, help: false, fxPicker: null, fxChainWin: null, vstPicker: null, missingOpen: false, musicWin: null, midiWin: null, menu: null, openingProject: false });
  (await import('./Palette')).setPaletteOpen(false);
  (await import('../state/stems')).closeStems();
  settle();
  vi.restoreAllMocks();
});

const ticks = async () => { for (let i = 0; i < 3; i++) await Promise.resolve(); settle(); };
/** The engine sends a state with `mut` applied to the project (what a state event carries). */
const engineSends = (mut: (p: StateDto['project'] & Record<string, unknown>) => void) => {
  const dto = structuredClone(base); mut(dto.project as never); store.replaceState(dto); settle();
};
/** The engine's state as it really is (the bad item gone, as after an undo). */
const engineFixed = () => { store.replaceState(structuredClone(base)); settle(); };
const audioClip = () => store.S.project.clips.find(c => c.kind === 'audio')!;
const synth = () => store.S.project.channels!.find(c => c.kind === 'synth')!;

/** The rest of the app still reacts: a panel opens and closes, the status line follows (unless it is the failed part). */
function stillAlive(readout = true) {
  ui.setU('pickOpen', false);
  expect(document.querySelector('.browser')).toBeNull();
  ui.setU('pickOpen', true);
  expect(document.querySelector('.browser')).not.toBeNull();
  if (readout) { ui.say('still alive'); expect(document.querySelector('[data-readout]')!.textContent).toBe('still alive'); }
}
/** Ctrl+S still saves the project. */
async function stillSaves() {
  const save = vi.spyOn(mock, 'projectSave');
  fireEvent.keyDown(window, { key: 's', code: 'KeyS', ctrlKey: true });
  await nextTask(); await syncEngine(mock, store, edit);
  expect(save).toHaveBeenCalled();
  expect(store.S.dirty).toBe(false);
}
/** Click "Try again" inside `where` while it still shows a failure. */
function tryAgain(where: Element) {
  const b = [...where.querySelectorAll('button')].find(x => x.textContent === 'Try again');
  if (b) { fireEvent.click(b); settle(); }
}
const alertIn = (sel: string) => document.querySelector(sel + ' [role=alert], ' + sel + '[role=alert]');
const errs = () => new Set(reported.map(e => (e as Error).message));

interface Case {
  /** Opens the surface (and anything it needs). */
  open?: () => void | Promise<void>;
  /** Makes it fail (the engine's state, or a bomb), and undoes that. */
  cause: { make: () => void; undo: () => void };
  /** Where the failure must show (the surface's own element), and what of its frame must stay. */
  where: string; frame?: string; text: string;
  /** The surface working again. */
  ok: () => void;
  /** The cause is not reactive: only Try again builds it again. */
  retry?: boolean;
  readout?: boolean;
}
/** A bomb in `key`; the language flipped and back makes every text be looked up again, so it goes off where it is shown. */
const bombed = (key: string) => ({ make: () => { bomb.keys.add(key); relabel(); }, undo: () => { bomb.keys.delete(key); } });
const relabel = () => { setLang('pl'); settle(); setLang('en'); settle(); };

async function journey(c: Case) {
  await c.open?.(); settle(); await ticks();
  const listeners = new Map(listening), tasks = frames.live.size;
  const box = (document.querySelector(c.where) as HTMLElement | null);
  const place = box && { left: box.style.left, top: box.style.top, z: box.style.zIndex };
  c.cause.make(); settle(); await ticks();
  // a bomb only goes off when its surface renders that text again: build it again
  if (!alertIn(c.where)) { const el = document.querySelector(c.where); if (el) tryAgain(el); await ticks(); }
  if (!alertIn(c.where) && c.retry) { c.cause.undo(); await c.open?.(); c.cause.make(); settle(); await ticks(); }
  const alert = alertIn(c.where);
  expect(alert, 'failure shown in ' + c.where).not.toBeNull();
  expect(alert!.textContent).toContain(c.text);
  expect(reported.length, 'reported as uncaught').toBeGreaterThan(0);
  expect(document.querySelectorAll('.apperr').length, 'not the whole app').toBe(0);
  if (c.frame) expect(document.querySelector(c.frame), 'frame kept: ' + c.frame).not.toBeNull();
  stillAlive(c.readout !== false);
  await stillSaves();
  // back: the cause undone (and Try again when nothing reactive changed)
  c.cause.undo(); settle(); await ticks();
  const el = document.querySelector(c.where);
  if (el && alertIn(c.where)) { tryAgain(el); await ticks(); }
  expect(alertIn(c.where), 'recovered ' + c.where).toBeNull();
  c.ok();
  // nothing leaked or doubled, the window where it was
  expect(observed.els().filter(e => !e.isConnected), 'size observers left on removed elements').toEqual([]);
  expect(new Map(listening)).toEqual(listeners);
  expect(frames.live.size).toBe(tasks);
  const after = document.querySelector(c.where) as HTMLElement | null;
  if (place && after) expect({ left: after.style.left, top: after.style.top, z: after.style.zIndex }).toEqual(place);
}

describe('panels in the main window', () => {
  it('Playlist', async () => journey({
    cause: bombed('a.pl.addTrack'), where: '.workrow', text: 'Playlist stopped because of an error', retry: true,
    ok: () => { expect(document.querySelector('.plwrap .addTrack')!.textContent).toContain('Add track'); },
  }));

  it('Browser: a tab fails alone, the tabs still switch', async () => {
    await journey({
      cause: { make: () => engineSends(p => { (p.patterns[0] as { notes: unknown }).notes = null; }), undo: engineFixed },
      where: '.browser', frame: '.browser .btabs', text: 'Browser (Patterns) stopped',
      ok: () => { expect(document.querySelectorAll('.browser .pitem').length).toBe(store.S.project.patterns.length); },
    });
    engineSends(p => { (p.patterns[0] as { notes: unknown }).notes = null; }); await ticks();
    expect(alertIn('.browser')).not.toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'LIBRARY' }));
    expect(alertIn('.browser')).toBeNull(); // another tab works
    engineFixed();
  });

  it('Mixer (docked): the failure keeps its dock frame', async () => journey({
    cause: { make: () => engineSends(p => { (p.inserts[1] as { name: unknown }).name = null; }), undo: engineFixed },
    where: '.mixdock', frame: '.mixdock .mixhd', text: 'Mixer stopped because of an error',
    ok: () => { expect(document.querySelectorAll('.mixdock .strip').length).toBe(store.S.project.inserts.length); },
  }));

  it('Toolbar: the transport fails as one line in its place; the window buttons and the other parts stay', async () => {
    await journey({
      cause: { make: () => engineSends(p => { (p as { bpm: unknown }).bpm = null; }), undo: engineFixed },
      where: '.tbtempo', text: 'Transport: error',
      ok: () => { expect(document.querySelector('.tbtempo .bpm')!.textContent).toBe(store.S.project.bpm.toFixed(2)); },
    });
  });

  it('Toolbar: the menus', async () => journey({
    cause: bombed('a.cmd.title'), where: '.tbmenus', text: 'Menus: error', retry: true, readout: false,
    ok: () => { expect(screen.getByRole('button', { name: /Find a command/ })).toBeTruthy(); },
  }));

  it('Toolbar: the panel buttons', async () => journey({
    cause: bombed('a.tb.browserTip'), where: '.tbpanels', text: 'Panel buttons: error', retry: true,
    ok: () => { expect(document.querySelector('.tbpanels .panelbtn')).not.toBeNull(); },
  }));
});

/** A failure inside the Piano roll's content: its snap chip's tooltip, rendered only there. */
const rollBomb = bombed('b.pe.snapTitle');

describe('windows', () => {
  it('Piano roll (floating): content failure, the title bar stays and still closes it', async () => {
    await journey({
      open: () => pat.openRoll(),
      // (an invalid key from the engine no longer breaks it: the song key reads any scale id)
      cause: rollBomb, retry: true,
      where: '.rollwin', frame: '.rollwin .winhd .closebtn', text: 'Piano roll stopped because of an error',
      ok: () => { expect(document.querySelector('.rollwin .keybadge')).not.toBeNull(); },
    });
    rollBomb.make(); tryAgain(document.querySelector('.rollwin')!); await ticks();
    expect(alertIn('.rollwin')).not.toBeNull();
    fireEvent.click(document.querySelector('.rollwin .winhd .closebtn')!);
    expect(document.querySelector('.rollwin')).toBeNull();
    expect(ui.U.rollWin).toBeNull();
    rollBomb.undo();
  });

  it('Piano roll: Try again while it still fails, then its cause undone: the new note area is the one measured', async () => {
    pat.openRoll(); settle(); await ticks();
    const area0 = document.querySelector('.rollwin .parea')!;
    expect(observed.els()).toContain(area0);
    rollBomb.make(); tryAgain(document.querySelector('.rollwin')!); await ticks();
    expect(alertIn('.rollwin'), 'failing').not.toBeNull();
    tryAgain(document.querySelector('.rollwin')!); await ticks();
    expect(alertIn('.rollwin'), 'still failing').not.toBeNull();
    rollBomb.undo(); tryAgain(document.querySelector('.rollwin')!); await ticks();
    expect(alertIn('.rollwin')).toBeNull();
    const area = document.querySelector('.rollwin .parea')!;
    expect(area).not.toBe(area0); // built again
    // the canvases follow the size of the area on screen, and nothing watches the removed one
    expect(observed.els()).toContain(area);
    expect(observed.els()).toContain(document.querySelector('.rollwin .lanecv'));
    expect(observed.els().filter(e => !e.isConnected)).toEqual([]);
    pat.closeRoll(); settle();
  });

  it('Piano roll: a failure of its title bar keeps a minimal frame in its place that moves and closes', async () => {
    pat.openRoll(); settle();
    const g0 = { ...ui.U.rollWin! };
    bomb.keys.add('b.pr.chTitle'); // the channel picker in its title bar
    pat.closeRoll(); settle(); pat.openRoll(); settle(); await ticks();
    const win = document.querySelector<HTMLElement>('.rollwin.failwin')!;
    expect(win).not.toBeNull();
    expect(win.querySelector('.title')!.textContent).toBe('Piano roll');
    expect(win.style.left).toBe(ui.U.rollWin!.x + 'px');
    fireEvent.pointerDown(win.querySelector('.winhd')!, { button: 0, clientX: 300, clientY: 100 });
    fireEvent.pointerMove(window, { clientX: 340, clientY: 130 });
    fireEvent.pointerUp(window, {});
    settle();
    expect(ui.U.rollWin!.x).toBe(g0.x + 40);
    bomb.keys.clear();
    tryAgain(win); await ticks();
    expect(document.querySelector('.rollwin.failwin')).toBeNull();
    expect(document.querySelector<HTMLElement>('.rollwin')!.style.left).toBe(g0.x + 40 + 'px');
    pat.closeRoll(); settle();
  });

  it('Mixer (floating, then maximized)', async () => {
    const dk = await import('../state/docking');
    const insertNameNull = { make: () => engineSends(p => { (p.inserts[1] as { name: unknown }).name = null; }), undo: engineFixed };
    await journey({
      open: () => { if (!document.querySelector('.mixwin')) dk.floatPane('mix'); },
      cause: insertNameNull, where: '.mixwin', frame: '.mixwin .winhd .closebtn', text: 'Mixer stopped because of an error',
      ok: () => { expect(document.querySelectorAll('.mixwin .strip').length).toBe(store.S.project.inserts.length); },
    });
    await journey({
      open: () => ui.setU('mixView', true),
      cause: insertNameNull, where: '.mixview', frame: '.mixview .mixhd', text: 'Mixer stopped because of an error',
      ok: () => { expect(document.querySelectorAll('.mixview .strip').length).toBe(store.S.project.inserts.length); },
    });
    ui.setU('mixView', false);
    dk.resetLayout(); settle();
  });

  it('Piano roll (docked): the failure keeps its dock frame', async () => {
    const dk = await import('../state/docking');
    await journey({
      open: () => { pat.openRoll(); settle(); if (!document.querySelector('.rollwin.docked')) dk.dockPaneTo('roll', 'bottom'); },
      cause: rollBomb, retry: true,
      where: '.rollwin.docked', frame: '.rollwin.docked .winhd .closebtn', text: 'Piano roll stopped because of an error',
      ok: () => { expect(document.querySelector('.rollwin.docked .keybadge')).not.toBeNull(); },
    });
    dk.resetLayout(); pat.closeRoll(); settle();
  });

  it('Channel rack', async () => journey({
    open: () => pat.openRack(),
    cause: { make: () => engineSends(p => { for (const s of p.sources) Object.assign(s, { sr: null, ch: null, path: null }); }), undo: engineFixed },
    where: '.rackwin', frame: '.rackwin .winhd .closebtn', text: 'Channel rack stopped because of an error',
    ok: () => { expect(document.querySelectorAll('.rackwin .crow').length).toBeGreaterThan(1); },
  }));

  it('Effect window', async () => {
    const fx = store.S.project.inserts.flatMap(m => m.fx).find(f => f.type === 'delay' || f.type === 'reverb' || f.type === 'comp')!;
    await journey({
      open: () => A.openPlug(fx.id),
      cause: { make: () => engineSends(p => { for (const m of p.inserts) for (const f of m.fx) if (f.id === fx.id) (f as { params: unknown }).params = null; }), undo: engineFixed },
      where: '.fxwin', frame: '.fxwin .winhd .closebtn', text: 'Effect window stopped because of an error',
      ok: () => { expect(document.querySelector('.fxwin .knob, .fxwin canvas')).not.toBeNull(); },
    });
  });

  it('Channel window', async () => {
    const cw = await import('./ChannelWindow');
    await journey({
      open: () => { if (!cw.chanWin()) cw.toggleChannelWindow(synth().id, true); },
      cause: { make: () => engineSends(p => { for (const c of p.channels!) if (c.kind === 'synth') (c.synth as { params: unknown }).params = null; }), undo: engineFixed },
      where: '.chanwin', frame: '.chanwin .winhd .closebtn', text: 'Channel window stopped because of an error',
      ok: () => { expect(document.querySelector('.chanwin .chknobs')).not.toBeNull(); },
    });
    cw.closeChannelWindow(); settle();
  });

  it('Sample editor', async () => {
    await journey({
      open: () => A.openSampleEditor({ kind: 'clip', id: audioClip().id }),
      cause: { make: () => engineSends(p => { for (const c of p.clips) if (c.kind === 'audio') Object.assign(c, { offset: null, gain: null }); }), undo: engineFixed },
      where: '.smpwin', frame: '.smpwin .winhd .closebtn', text: 'Sample editor stopped because of an error',
      ok: () => { expect(document.querySelector('.smpwin .smpknobs')).not.toBeNull(); },
    });
    A.closeEditor(); settle();
  });

  it('Pitch editor', async () => {
    await journey({
      open: () => A.openPitchEditor(audioClip().id),
      cause: bombed('d.pitch.key'), retry: true,
      where: '.pitchwin', frame: '.pitchwin .winhd .closebtn', text: 'Pitch editor stopped because of an error',
      ok: () => { expect(document.querySelector('.pitchwin .ptools')).not.toBeNull(); },
    });
    A.closePitchEditor(); settle();
  });

  it('Notebook', async () => {
    const nb = await import('../state/notebook');
    await journey({
      open: () => nb.openNotebook(),
      cause: { make: () => engineSends(p => { Object.assign(p, { notebook: { pages: [{ id: 900, name: 'Verse', text: null, startBar: 1 }] } }); }), undo: engineFixed },
      where: '.nbwin', frame: '.nbwin .winhd .closebtn', text: 'Notebook stopped because of an error',
      ok: () => { expect(document.querySelector('.nbwin textarea')).not.toBeNull(); },
    });
    nb.closeNotebook(); settle();
  });
});

describe('dialogs: a failed dialog says so, closes with Esc, and opens again working', () => {
  const dialogs: [string, string, () => void | Promise<void>, () => boolean][] = [
    ['Settings', 'd.set.tab.general.desc', () => ui.setU('settings', true), () => !!document.querySelector('.setdlg')],
    ['Export', 'a.ex.what', () => ui.setU('exportOpen', true), () => !!document.querySelector('.exdlg')],
    ['Keyboard shortcuts', 'a.sc.1', () => ui.setU('help', true), () => !!document.querySelector('.helpdlg')],
    ['Find a command', 'a.cmd.placeholder', async () => (await import('./Palette')).setPaletteOpen(true), () => !!document.querySelector('.cmddlg')],
    ['Effect picker', 'c.picker.compressor', () => ui.setU('fxPicker', { insId: 1, x: 100, y: 100 }), () => !!document.querySelector('.fxpick, [data-fxpicker], .dialog:not(.faildlg)')],
    ['Effect chains', 'e.fxc.search', () => ui.setU('fxChainWin', { insId: 1, x: 100, y: 100 }), () => !document.querySelector('.faildlg')],
    ['Add plugin', 'd.set.search', () => ui.setU('vstPicker', { insId: 1 }), () => !document.querySelector('.faildlg')],
    ['Missing audio', 'd.miss.allFound', () => ui.setU('missingOpen', true), () => !!document.querySelector('.missdlg')],
    ['Tempo and key analysis', 'd.music.analyzing', () => A.openMusicAnalysis({ clipId: audioClip().id }), () => !!document.querySelector('[data-music-dialog]')],
    ['Extract stems', 'd.stems.what', async () => (await import('../state/stems')).openStems(audioClip().id), () => !!document.querySelector('[data-stems-dialog]')],
    ['MIDI import', 'd.midi.cancel', () => ui.setU('midiWin', { path: 'C:/Music/Groove.mid' }), () => !!document.querySelector('[data-midi-dialog]')],
  ];
  for (const [name, key, open, ok] of dialogs) {
    it(name, async () => {
      bomb.keys.add(key);
      await open(); settle(); await ticks(); await nextTask(); await ticks();
      const dlg = screen.getByRole('alertdialog', { name });
      expect(dlg.textContent).toContain(name + ' stopped because of an error');
      stillAlive();
      await stillSaves();
      // Esc closes the failed dialog, not a window behind it
      await new Promise(r => setTimeout(r, 0));
      expect(document.activeElement).toBe(dlg);
      fireEvent.keyDown(dlg, { key: 'Escape' });
      settle(); await ticks();
      expect(screen.queryByRole('alertdialog')).toBeNull();
      bomb.keys.clear();
      await open(); settle(); await ticks(); await nextTask(); await ticks();
      expect(screen.queryByRole('alertdialog')).toBeNull();
      expect(ok()).toBe(true);
      fireEvent.keyDown(window, { key: 'Escape' }); settle();
    });
  }

  it('the unsaved-changes question: closing its failure answers Cancel (nothing is lost)', async () => {
    const C = await import('../ui/Confirm');
    bomb.keys.add('d.ui.unsaved.body');
    store.setS('dirty', true);
    const answer = C.confirmUnsaved('closing');
    settle(); await ticks();
    fireEvent.click(screen.getByRole('alertdialog').querySelector('.acts .smallbtn:not(.primary)')!);
    await expect(answer).resolves.toBe('cancel');
  });

  it('Welcome', async () => {
    const up = await import('../state/updates');
    await up.initUpdates();
    bomb.keys.add('d.welcome.body');
    up.offerWelcome(true); settle(); await ticks();
    const dlg = screen.getByRole('alertdialog', { name: 'Welcome to Synkopa' });
    fireEvent.click(dlg.querySelector('.acts .smallbtn:not(.primary)')!);
    settle();
    expect(screen.queryByRole('alertdialog')).toBeNull();
  });
});

describe('small surfaces', () => {
  it('a context menu that fails says so in a small box where it opened; the next menu works', async () => {
    bomb.keys.add('d.ui.color');
    ui.openMenu(220, 140, [{ t: 'swatches', current: '#fff', pick: () => {} }]);
    settle(); await ticks();
    const box = document.querySelector<HTMLElement>('.menu.ctxmenu')!;
    expect(box.querySelector('.panelerr.small')!.textContent).toContain('Menu: error');
    expect(box.style.left).toBe('220px');
    stillAlive();
    fireEvent.keyDown(window, { key: 'Escape' }); settle();
    expect(document.querySelector('.menu.ctxmenu')).toBeNull();
    bomb.keys.clear();
    ui.openMenu(220, 140, [{ t: 'item', label: 'Rename', act: () => {} }]);
    settle();
    expect(screen.getByRole('menuitem', { name: /Rename/ })).toBeTruthy();
    ui.closeMenu(); settle();
  });

  it('a message toast that fails is one line in the toast; × still dismisses it', async () => {
    const aw = await import('../state/audioWatch');
    // the output device is gone: the toast says so; then WASAPI reports an error, whose text fails to render
    let push!: (st: unknown) => void;
    await aw.watchAudio(async () => ({ host: 'wasapi', running: false, unavailableOutput: 'Speakers' }) as never, async cb => { push = cb as never; }, () => {});
    settle();
    expect(document.querySelector('.audio-toast')).not.toBeNull();
    bomb.keys.add('d.audio.wasapi.error');
    push({ host: 'wasapi', running: false, error: 'device busy' }); settle(); await ticks();
    const chip = document.querySelector('.audio-toast .panelerr.chip')!;
    expect(chip.textContent).toContain('Message: error');
    stillAlive();
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss audio notification' }));
    settle();
    expect(document.querySelector('.audio-toast')).toBeNull();
  });
});

describe('the whole app', () => {
  it('a failure outside every surface fills the window with the message; Ctrl+S still saves; Try again rebuilds it', async () => {
    bomb.keys.add('d.act.opening'); // the "opening a project" overlay, drawn by the app itself
    ui.setU('openingProject', true); settle(); await ticks();
    const alert = document.querySelector('.apperr')!;
    expect(alert).not.toBeNull();
    expect(alert.textContent).toContain('Synkopa’s window stopped because of an error');
    expect(errs()).toContain('render bug: d.act.opening');
    ui.setU('openingProject', false); settle();
    await stillSaves();
    bomb.keys.clear();
    tryAgain(alert); await ticks();
    expect(document.querySelector('.apperr')).toBeNull();
    expect(document.querySelector('.plwrap')).not.toBeNull();
  });

  it('the app built again asks once before the window closes: the one it replaced let go of the close request', async () => {
    // a desktop window (the mock has none outside `?chrome`), counting the close guards it holds
    let guards = 0;
    const win = {
      minimize: async () => {}, toggleMaximize: async () => {}, close: async () => {}, isMaximized: async () => false, startDragging: async () => {},
      isFullscreen: async () => false, setFullscreen: async () => {}, onResized: async () => () => {}, setZoom: async () => false,
      onCloseRequested: async () => { guards++; return () => { guards--; }; },
    };
    const had = Object.getOwnPropertyDescriptor(mock, 'win')!;
    Object.defineProperty(mock, 'win', { value: win, configurable: true, writable: true });
    try {
      for (let i = 0; i < 2; i++) {
        bomb.keys.add('d.act.opening');
        ui.setU('openingProject', true); settle(); await ticks();
        // the cause gone without anything reactive changing: only Try again builds the app again
        bomb.keys.clear();
        const app = document.querySelector('.plwrap');
        tryAgain(document.querySelector('.apperr')!); await ticks(); await nextTask();
        ui.setU('openingProject', false); settle();
        expect(document.querySelector('.apperr')).toBeNull();
        expect(document.querySelector('.plwrap'), 'built again').not.toBe(app);
        expect(guards, 'close guards after rebuild ' + (i + 1)).toBe(1);
      }
    } finally { Object.defineProperty(mock, 'win', had); }
  });
});
