// The modal keyboard contract (ui/modal.ts): Tab stays in the dialog on top, Esc closes it from
// anywhere unless a control inside used Esc itself, the app's shortcuts stay off, and the focus
// goes in on open and back to the opener on close — also with dialogs stacked.
import { fireEvent } from '@solidjs/testing-library';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { modal, modalOpen, modalOwnsKey, resetModals, tabStops } from './modal';

/** Let the registration settle (microtask) and the removal observer run. */
const settle = () => new Promise(r => setTimeout(r, 0));

function opener(label = 'Open') {
  const b = document.createElement('button');
  b.textContent = label;
  document.body.append(b);
  b.focus();
  return b;
}

/** A veil with a dialog in it: the buttons named, the one in `auto` marked data-autofocus. */
function dialog(names: string[], opts: { z?: number; auto?: string; closable?: boolean } = {}) {
  const veil = document.createElement('div');
  veil.className = 'overlay';
  if (opts.z != null) veil.style.zIndex = String(opts.z);
  const el = document.createElement('div');
  el.className = 'dialog';
  for (const n of names) {
    const b = document.createElement('button');
    b.textContent = n;
    if (n === opts.auto) b.dataset.autofocus = '';
    el.append(b);
  }
  veil.append(el);
  const close = vi.fn(() => veil.remove());
  modal(el, opts.closable === false ? {} : { close });
  document.body.append(veil);
  const btn = (n: string) => Array.from(el.querySelectorAll('button')).find(b => b.textContent === n)!;
  return { el, veil, close, btn };
}

afterEach(() => {
  resetModals();
  document.body.innerHTML = '';
});

describe('modal dialogs', () => {
  it('marks the box as a modal dialog and focuses the data-autofocus control', async () => {
    opener();
    const d = dialog(['Cancel', 'Save'], { auto: 'Save' });
    await settle();
    expect(d.el.getAttribute('role')).toBe('dialog');
    expect(d.el.getAttribute('aria-modal')).toBe('true');
    expect(d.el.tabIndex).toBe(-1);
    expect(document.activeElement).toBe(d.btn('Save'));
  });

  it('without a data-autofocus control, focuses the dialog itself', async () => {
    opener();
    const d = dialog(['A', 'B']);
    await settle();
    expect(document.activeElement).toBe(d.el);
  });

  it('Tab from the last control wraps to the first; Shift+Tab from the first wraps to the last', async () => {
    opener();
    const d = dialog(['A', 'B', 'C'], { auto: 'C' });
    await settle();
    fireEvent.keyDown(d.btn('C'), { key: 'Tab' });
    expect(document.activeElement).toBe(d.btn('A'));
    fireEvent.keyDown(d.btn('A'), { key: 'Tab', shiftKey: true });
    expect(document.activeElement).toBe(d.btn('C'));
  });

  it('Tab between the ends is left to the browser (its order is right there)', async () => {
    opener();
    const d = dialog(['A', 'B', 'C'], { auto: 'B' });
    await settle();
    const ev = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true });
    d.btn('B').dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(false);
  });

  it('a control that uses Tab itself (preventDefault) keeps it', async () => {
    opener();
    const d = dialog(['A', 'B'], { auto: 'B' });
    await settle();
    d.btn('B').addEventListener('keydown', e => e.preventDefault());
    fireEvent.keyDown(d.btn('B'), { key: 'Tab' });
    expect(document.activeElement).toBe(d.btn('B'));
  });

  it('Tab with the focus outside the dialog (on the page) goes into it', async () => {
    opener();
    const d = dialog(['A', 'B']);
    await settle();
    (document.activeElement as HTMLElement).blur();
    fireEvent.keyDown(document.body, { key: 'Tab' });
    expect(document.activeElement).toBe(d.btn('A'));
    (document.activeElement as HTMLElement).blur();
    fireEvent.keyDown(document.body, { key: 'Tab', shiftKey: true });
    expect(document.activeElement).toBe(d.btn('B'));
  });

  it('focus moved out of the dialog by code comes back', async () => {
    const o = opener();
    const d = dialog(['A', 'B'], { auto: 'B' });
    await settle();
    o.focus();
    expect(document.activeElement).toBe(d.btn('B'));
  });

  it('Esc closes it, with the focus inside or outside', async () => {
    opener();
    let d = dialog(['A'], { auto: 'A' });
    await settle();
    fireEvent.keyDown(d.btn('A'), { key: 'Escape' });
    expect(d.close).toHaveBeenCalledTimes(1);
    await settle();
    d = dialog(['A']);
    await settle();
    (document.activeElement as HTMLElement).blur();
    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(d.close).toHaveBeenCalledTimes(1);
  });

  it('Esc used by a control inside (a list, an edited name) does not close the dialog', async () => {
    opener();
    const d = dialog(['A'], { auto: 'A' });
    await settle();
    d.btn('A').addEventListener('keydown', e => e.stopPropagation());
    fireEvent.keyDown(d.btn('A'), { key: 'Escape' });
    expect(d.close).not.toHaveBeenCalled();
  });

  it('a dialog without close ignores Esc', async () => {
    opener();
    const d = dialog(['A'], { auto: 'A', closable: false });
    await settle();
    fireEvent.keyDown(d.btn('A'), { key: 'Escape' });
    expect(d.veil.isConnected).toBe(true);
  });

  it('keeps the app shortcuts off while open, and gives the focus back to the opener on close', async () => {
    const o = opener();
    const d = dialog(['A'], { auto: 'A' });
    await settle();
    expect(modalOpen()).toBe(true);
    expect(modalOwnsKey(new KeyboardEvent('keydown', { key: ' ' }))).toBe(true);
    fireEvent.keyDown(d.btn('A'), { key: 'Escape' });
    await settle();
    expect(modalOpen()).toBe(false);
    expect(modalOwnsKey(new KeyboardEvent('keydown', { key: ' ' }))).toBe(false);
    expect(document.activeElement).toBe(o);
  });

  it('lets the keys a dialog names through (passKeys)', async () => {
    opener();
    const veil = document.createElement('div'), el = document.createElement('div');
    veil.append(el);
    modal(el, { passKeys: e => e.ctrlKey && e.key === 's' });
    document.body.append(veil);
    await settle();
    expect(modalOwnsKey(new KeyboardEvent('keydown', { key: 's', ctrlKey: true }))).toBe(false);
    expect(modalOwnsKey(new KeyboardEvent('keydown', { key: 'Delete' }))).toBe(true);
  });

  it('stacked: the inner dialog owns Tab and Esc; closing it returns the focus to its opener in the outer one', async () => {
    opener();
    const outer = dialog(['Settings A', 'Settings B'], { auto: 'Settings B' });
    await settle();
    const inner = dialog(['Cancel', 'Save'], { auto: 'Save', z: 300 });
    await settle();
    expect(document.activeElement).toBe(inner.btn('Save'));
    fireEvent.keyDown(inner.btn('Save'), { key: 'Tab' });
    expect(document.activeElement).toBe(inner.btn('Cancel'));
    outer.btn('Settings A').focus(); // code pulls the focus into the dialog underneath: it comes back
    expect(inner.el.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(document.activeElement!, { key: 'Escape' });
    expect(inner.close).toHaveBeenCalledTimes(1);
    expect(outer.close).not.toHaveBeenCalled();
    await settle();
    expect(document.activeElement).toBe(outer.btn('Settings B'));
    expect(modalOpen()).toBe(true);
  });

  it('the dialog drawn on top owns the keyboard, whichever opened first', async () => {
    opener();
    const high = dialog(['High'], { auto: 'High', z: 300 });
    await settle();
    const low = dialog(['Low'], { auto: 'Low', z: 250 });
    await settle();
    expect(document.activeElement).toBe(high.btn('High'));
    fireEvent.keyDown(document.activeElement!, { key: 'Escape' });
    expect(high.close).toHaveBeenCalledTimes(1);
    expect(low.close).not.toHaveBeenCalled();
    await settle();
    expect(document.activeElement).toBe(low.btn('Low'));
  });
});

describe('tabStops', () => {
  it('skips disabled, hidden and tabindex=-1 controls and keeps one stop per radio group', () => {
    const el = document.createElement('div');
    el.innerHTML = `
      <button>a</button><button disabled>b</button><button tabindex="-1">c</button>
      <div hidden><button>d</button></div>
      <input type="radio" name="g" value="1"><input type="radio" name="g" value="2" checked>
      <input type="radio" name="h" value="1"><input type="radio" name="h" value="2">
      <input type="text"><a href="#x">link</a>`;
    document.body.append(el);
    const s = tabStops(el).map(x => x.outerHTML);
    expect(s).toEqual([
      '<button>a</button>',
      '<input type="radio" name="g" value="2" checked="">',
      '<input type="radio" name="h" value="1">',
      '<input type="text">',
      '<a href="#x">link</a>',
    ]);
  });
});
