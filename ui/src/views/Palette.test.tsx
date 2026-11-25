// The command palette (Ctrl+K): a keyboard-first view over the app menus.
import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setU } from '../state/ui';
import { settle } from '../test/settle';
import { Palette, paletteOpen, setCommandMenus, setPaletteOpen } from './Palette';
import { modalOwnsKey } from '../ui/modal';

const acts = { newP: vi.fn(), open: vi.fn(), exp: vi.fn(), locked: vi.fn() };
beforeEach(() => {
  for (const f of Object.values(acts)) f.mockClear();
  setU('sel', []);
  // the menu bar registers its menus like this; the palette reads them when it opens
  setCommandMenus([
    ['a.mn.file' as never, () => [
      { t: 'item', label: 'New project', kbd: 'Ctrl+N', act: acts.newP },
      { t: 'item', label: 'Open project', kbd: 'Ctrl+O', act: acts.open },
      { t: 'sub', label: 'Export', items: [{ t: 'item', label: 'Audio file', act: acts.exp }] },
      { t: 'item', label: 'Locked thing', act: acts.locked, disabled: true },
    ]],
  ]);
  render(() => <Palette />);
  setPaletteOpen(true);
  settle();
});
afterEach(() => setPaletteOpen(false));

const input = () => screen.getByRole('searchbox') as HTMLInputElement;
const type = (s: string) => { input().value = s; fireEvent.input(input()); settle(); };
const key = (k: string, init: KeyboardEventInit = {}) => { fireEvent.keyDown(input(), { key: k, ...init }); settle(); };
const selected = () => screen.getAllByRole('option').find(o => o.getAttribute('aria-selected') === 'true')?.textContent ?? '';

describe('Palette', () => {
  it('opens as a dialog listing every command, with submenu paths and shortcuts, the first one selected', async () => {
    expect(screen.getByRole('dialog')).toBeTruthy();
    const names = screen.getAllByRole('option').map(o => o.querySelector('.name')!.textContent);
    expect(names).toEqual(['New project', 'Open project', 'Audio file', 'Locked thing']);
    expect(screen.getByText('Export')).toBeTruthy(); // the submenu path, as a badge
    expect(selected()).toContain('New project');
    await Promise.resolve(); // focus is moved in a microtask
    expect(document.activeElement).toBe(input());
  });

  it('typing filters by every word, in any order', () => {
    type('file audio');
    expect(screen.getAllByRole('option').map(o => o.querySelector('.name')!.textContent)).toEqual(['Audio file', expect.stringContaining('file audio')]);
  });

  it('arrows move the selection (wrapping) and Enter runs it, closing the palette', () => {
    key('ArrowDown');
    expect(selected()).toContain('Open project');
    key('ArrowUp'); key('ArrowUp');
    expect(selected()).toContain('Locked thing');
    key('ArrowDown'); key('ArrowDown');
    key('Enter');
    expect(acts.open).toHaveBeenCalledOnce();
    expect(paletteOpen()).toBe(false);
  });

  it('a disabled command does not run', () => {
    fireEvent.click(screen.getByText('Locked thing'));
    expect(acts.locked).not.toHaveBeenCalled();
    expect(paletteOpen()).toBe(true);
  });

  it('a click runs a command', () => {
    fireEvent.click(screen.getByText('Audio file'));
    expect(acts.exp).toHaveBeenCalledOnce();
    expect(paletteOpen()).toBe(false);
  });

  it('Escape clears the query first, then closes', () => {
    type('open');
    key('Escape');
    expect(input().value).toBe('');
    expect(paletteOpen()).toBe(true);
    key('Escape');
    expect(paletteOpen()).toBe(false);
  });

  it('with no match it offers to ask instead, and keys never reach the app shortcuts', () => {
    type('how do I sidechain');
    // App.tsx's key handler asks the open modal first (ui/modal.ts)
    expect(modalOwnsKey(new KeyboardEvent('keydown', { key: 'Delete' }))).toBe(true);
    const opts = screen.getAllByRole('option');
    expect(opts).toHaveLength(1);
    expect(opts[0].textContent).toContain('how do I sidechain');
  });

  it('Ctrl+K and a press outside close it', () => {
    key('k', { ctrlKey: true });
    expect(paletteOpen()).toBe(false);
    setPaletteOpen(true); settle();
    fireEvent.pointerDown(document.querySelector('.cmdover')!);
    expect(paletteOpen()).toBe(false);
  });

  it('reopening starts from an empty query', () => {
    type('open');
    setPaletteOpen(false); settle();
    setPaletteOpen(true); settle();
    expect(input().value).toBe('');
    expect(screen.getAllByRole('option')).toHaveLength(4);
  });
});

describe('Palette, mounted while already open', () => {
  it('lists its commands (after the window is rebuilt with the palette open)', () => {
    cleanup(); // the palette of beforeEach: only the one mounted open below is in the page
    expect(paletteOpen()).toBe(true);
    render(() => <Palette />);
    settle();
    expect(screen.getByRole('dialog')).toBeTruthy();
    expect(screen.getAllByRole('option').map(o => o.querySelector('.name')!.textContent)).toContain('New project');
  });
});
