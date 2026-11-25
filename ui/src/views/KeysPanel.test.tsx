// The Synkopa Instruments window: its library of instrument cards (pick, keyboard, Esc back to the
// instrument), the instrument face's arrows, and knobs the instrument has nothing for.
import { fireEvent, render, screen } from '@solidjs/testing-library';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';
import type { Channel } from '../api/types';
import { syncEngine } from '../test/engine';
import { nextTask, settle } from '../test/settle';

vi.mock('../ui/Confirm', () => ({ confirmUnsaved: async () => 'discard', confirmRecover: async () => 'later' }));

let mock: MockBackend;
let store: typeof import('../state/store');
let P: typeof import('../state/pattern');
let KeysPanel: typeof import('./KeysPanel').KeysPanel;

type KeysChannel = Extract<Channel, { kind: 'keys' }>;
const keysChan = () => P.channels().find(c => c.kind === 'keys') as KeysChannel;
const engineInst = async () => ((await mock.getState()).project.channels?.find(c => c.kind === 'keys') as KeysChannel | undefined)?.keys.instrument;

beforeEach(async () => {
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  store = await import('../state/store');
  const edit = await import('../state/edit');
  P = await import('../state/pattern');
  ({ KeysPanel } = await import('./KeysPanel'));
  await syncEngine(mock, store, edit);
  if (!P.channels().some(c => c.kind === 'keys')) P.addKeys();
  await nextTask();
  settle();
});

async function open() {
  render(() => <KeysPanel c={keysChan()} />);
  for (let i = 0; i < 4; i++) { await nextTask(); settle(); }
}
const cards = () => [...document.querySelectorAll<HTMLElement>('.kcard')];
const cardNamed = (name: string) => cards().find(c => c.querySelector('.nm')?.textContent === name)!;

describe('Synkopa Instruments window', () => {
  it('names a new channel after its instrument, not the product', () => {
    expect(keysChan().name).toBe('Grand Piano');
  });

  it('the library lists every instrument as a card; a click makes the channel play it and stays in the library', async () => {
    await open();
    fireEvent.click(screen.getByRole('button', { name: /Instruments/ }));
    settle();
    const names = cards().map(c => c.querySelector('.nm')!.textContent);
    expect(names).toContain('Hollow Body Guitar');
    expect(names).toContain('Jazz Club Guitar');
    expect(cardNamed('Grand Piano').getAttribute('aria-selected')).toBe('true');
    // a downloadable instrument offers its download on the card
    expect(cardNamed('Hollow Body Guitar').querySelector('.kcard-dl')?.textContent).toMatch(/MB/);
    fireEvent.click(cardNamed('Hollow Body Guitar'));
    settle();
    expect(keysChan().keys.instrument).toBe('hollow-body-guitar');
    // still named after its instrument
    expect(keysChan().name).toBe('Hollow Body Guitar');
    // the edit reaches the engine (the mock answers on a later task)
    for (let i = 0; i < 10 && (await engineInst()) !== 'hollow-body-guitar'; i++) await new Promise(r => setTimeout(r, 20));
    expect(await engineInst()).toBe('hollow-body-guitar');
    expect(document.querySelector('.klib')).not.toBeNull();
  });

  it('the category filters narrow the cards', async () => {
    await open();
    fireEvent.click(screen.getByRole('button', { name: /Instruments/ }));
    settle();
    fireEvent.click(screen.getByRole('radio', { name: /Guitars/ }));
    settle();
    expect(cards().map(c => c.querySelector('.nm')!.textContent)).toEqual(['Hollow Body Guitar', 'Jazz Club Guitar']);
    fireEvent.click(screen.getByRole('radio', { name: /All/ }));
    settle();
  });

  it('arrows move between cards, Enter picks, Esc goes back to the instrument without leaving the panel', async () => {
    await open();
    fireEvent.click(screen.getByRole('button', { name: /Instruments/ }));
    settle();
    await Promise.resolve();
    const grid = document.querySelector('.kgrid')!;
    expect(document.activeElement).toBe(cardNamed(keysChan().keys.instrument === 'grand-piano' ? 'Grand Piano' : 'Hollow Body Guitar'));
    const from = cards().indexOf(document.activeElement as HTMLElement);
    fireEvent.keyDown(document.activeElement!, { key: 'ArrowRight' });
    expect(cards().indexOf(document.activeElement as HTMLElement)).toBe(from + 1);
    fireEvent.keyDown(document.activeElement!, { key: 'Enter' });
    settle();
    const picked = cards()[from + 1].querySelector('.nm')!.textContent;
    expect(keysChan().name).toBe(picked);
    // the status refresh after a pick keeps the cards (and the keyboard on them)
    for (let i = 0; i < 3; i++) { await nextTask(); settle(); }
    expect(document.activeElement?.classList.contains('kcard')).toBe(true);
    // Esc goes back to the face; the app's own Esc (which closes the window) never sees it
    const outside = vi.fn();
    window.addEventListener('keydown', outside);
    fireEvent.keyDown(grid.querySelector('.kcard')!, { key: 'Escape' });
    window.removeEventListener('keydown', outside);
    settle();
    expect(outside).not.toHaveBeenCalled();
    expect(document.querySelector('.klib')).toBeNull();
    expect(document.querySelector('.kface')).not.toBeNull();
  });

  it('the face steps to the next and previous instrument', async () => {
    P.setKeysInstrument(keysChan().id, 'grand-piano');
    settle();
    await open();
    fireEvent.click(screen.getByRole('button', { name: 'Next instrument' }));
    settle();
    expect(keysChan().keys.instrument).toBe('upright-piano');
    fireEvent.click(screen.getByRole('button', { name: 'Previous instrument' }));
    fireEvent.click(screen.getByRole('button', { name: 'Previous instrument' }));
    settle();
    // wraps around to the last instrument of the catalog
    expect(keysChan().keys.instrument).toBe('vibraphone');
  });

  it('a mono instrument dims Width and says why; one without release noises dims Noise', async () => {
    P.setKeysInstrument(keysChan().id, 'wurlitzer');
    settle();
    await open();
    const dim = [...document.querySelectorAll<HTMLElement>('.kna')];
    expect(dim.map(d => d.title)).toEqual([
      expect.stringMatching(/recorded in mono/),
      expect.stringMatching(/no key-release noises/),
    ]);
    P.setKeysInstrument(keysChan().id, 'grand-piano');
    settle(); await nextTask(); settle();
    expect(document.querySelectorAll('.kna').length).toBe(0);
  });

  it('switching instruments keeps the sound knobs and drops the old instrument\'s release and articulation', async () => {
    const c = keysChan();
    const keys = { ...c.keys, instrument: 'strings', release: 800, articulation: 25, tone: -40, width: 80, tune: 12 };
    const edit = await import('../state/edit');
    edit.commit([P.chanOp(c, { keys } as Partial<Channel>)], 'set');
    settle();
    P.setKeysInstrument(c.id, 'harp');
    settle();
    const k = keysChan().keys;
    expect(k.instrument).toBe('harp');
    expect(k.release).toBeUndefined();
    expect(k.articulation).toBeUndefined();
    expect([k.tone, k.width, k.tune]).toEqual([-40, 80, 12]);
  });
});

describe('KeysPanel failure paths', () => {
  it('a failed catalog fetch shows a Retry that fetches the catalog again', async () => {
    const spy = vi.spyOn(mock, 'keysStatus').mockRejectedValueOnce(new Error('catalog gone'));
    await open();
    expect(document.querySelector('.keysmsg.bad')?.textContent).toContain('catalog gone');
    spy.mockRestore();
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    for (let i = 0; i < 4; i++) { await nextTask(); settle(); }
    expect(document.querySelector('.keysmsg.bad')).toBeNull(); // the retry dismissed the banner
    expect(document.querySelector('.knm')).not.toBeNull(); // the face recovered with its instrument
  });
  it('a loaded-but-empty catalog says so instead of an empty face', async () => {
    const spy = vi.spyOn(mock, 'keysStatus').mockResolvedValue({ instruments: [] } as never);
    await open();
    expect(document.querySelector('.keysmsg')?.textContent).toContain('No instruments');
    spy.mockRestore();
  });
});
