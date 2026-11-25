// The key badge's picker: a pick keeps the pop open (root, then scale, at your own pace) and Esc
// closes it no matter where the keyboard lives — the roll's grid gets the keyboard after a pick,
// so the app's own Escape (which clears the note selection) must not see it.
import { render, fireEvent } from '@solidjs/testing-library';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';
import { syncEngine } from '../test/engine';
import { nextTask, settle } from '../test/settle';

vi.mock('../ui/Confirm', () => ({ confirmUnsaved: async () => 'discard', confirmRecover: async () => 'later' }));

let mock: MockBackend;
let KeyBadge: typeof import('./KeyBadge').KeyBadge;

beforeEach(async () => {
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  const store = await import('../state/store');
  const edit = await import('../state/edit');
  ({ KeyBadge } = await import('./KeyBadge'));
  await syncEngine(mock, store, edit);
  await nextTask();
  settle();
});

const breathe = async () => {
  for (let i = 0; i < 3; i++) { await nextTask(); settle(); }
};
const pop = () => document.querySelector('.keypop');
const openPicker = async () => {
  render(() => <KeyBadge />);
  await breathe();
  fireEvent.click(document.querySelector('.keybadge')!);
  await breathe();
};

describe('KeyBadge picker', () => {
  it('a pick keeps the pop open (root, then scale, at your own pace)', async () => {
    await openPicker();
    expect(pop()).not.toBeNull();
    fireEvent.click(document.querySelector('.rootgrid button')!);
    await breathe();
    expect(pop()).not.toBeNull(); // the root landed, the picker stays for the scale
    fireEvent.click(document.querySelector('.scalelist button')!);
    await breathe();
    expect(pop()).not.toBeNull();
  });

  it('Esc closes the picker and does not reach the app (the roll keeps its selection)', async () => {
    await openPicker();
    const seen: string[] = [];
    window.addEventListener('keydown', e => seen.push(e.key));
    fireEvent(document, new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await breathe();
    expect(pop()).toBeNull();
    expect(seen).toEqual([]); // stopped before the app's own Escape handler
  });

  it('after a pick the journey is: keyboard on the roll window, Esc there closes the picker before the roll sees it', async () => {
    await openPicker();
    // a pick, exactly like choosing a root in the roll's badge (the pop stays for the scale)
    fireEvent.click(document.querySelector('.rootgrid button')!);
    await breathe();
    expect(pop()).not.toBeNull();
    // `backToRoll` hands the keyboard to the roll's window; a stand-in with the same class and
    // a listener where the roll's own Escape handler (which clears the note selection) lives
    const roll = document.createElement('div');
    roll.className = 'win';
    roll.tabIndex = -1;
    document.body.appendChild(roll);
    roll.focus();
    const fromRoll: string[] = [];
    roll.addEventListener('keydown', e => fromRoll.push(e.key));
    // Esc on the focused roll window
    fireEvent(roll, new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await breathe();
    expect(pop()).toBeNull(); // the picker is dismissed
    expect(fromRoll).toEqual([]); // the roll's handler never ran: nothing can clear the selection
    roll.remove();
  });
});
