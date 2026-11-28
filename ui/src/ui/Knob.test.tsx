// Knob, MiniKnob, Segmented and NumField: the continuous controls of every effect and channel window.
import { fireEvent, render, screen } from '@solidjs/testing-library';
import { createSignal } from '../lib/reactive';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { settle } from '../test/settle';
import { resetDblCheck } from './drag';
import { Knob, MiniKnob, NumField, parseNum, Segmented } from './Knob';

beforeEach(() => resetDblCheck());

function knob(extra: Partial<Parameters<typeof Knob>[0]> = {}) {
  const [v, setV] = createSignal(0.5);
  const h = { onStart: vi.fn(), onChange: vi.fn((t: number) => setV(t)), onEnd: vi.fn(), onReset: vi.fn(), onWheel: vi.fn(), onMenu: vi.fn() };
  const r = render(() => <Knob t={v} text={() => Math.round(v() * 100) + '%'} label="Mix" key="mix" {...h} {...extra} />);
  const dial = r.container.querySelector('.dial') as HTMLElement;
  return { ...h, dial, v, r };
}

describe('Knob', () => {
  it('shows its label and value text, and follows the value', () => {
    const k = knob();
    expect(screen.getByText('Mix')).toBeTruthy();
    expect(screen.getByText('50%')).toBeTruthy();
    fireEvent.pointerDown(k.dial, { button: 0, clientY: 100 });
    fireEvent.pointerMove(window, { clientY: 85 });
    settle();
    expect(screen.getByText('60%')).toBeTruthy();
    fireEvent.pointerUp(window, {});
  });

  it('drags up to raise: start, changes, end; 150 px is the whole travel', () => {
    const k = knob();
    fireEvent.pointerDown(k.dial, { button: 0, clientY: 200 });
    expect(k.onStart).toHaveBeenCalledOnce();
    fireEvent.pointerMove(window, { clientY: 170 });
    expect(k.onChange).toHaveBeenLastCalledWith(expect.closeTo(0.7, 6));
    fireEvent.pointerMove(window, { clientY: 0 }); // far past the top: clamped
    expect(k.onChange).toHaveBeenLastCalledWith(1);
    fireEvent.pointerUp(window, {});
    expect(k.onEnd).toHaveBeenCalledOnce();
  });

  it('Ctrl or Shift drags fine (a fifth of the speed)', () => {
    const k = knob();
    fireEvent.pointerDown(k.dial, { button: 0, clientY: 200 });
    fireEvent.pointerMove(window, { clientY: 170, ctrlKey: true });
    expect(k.onChange).toHaveBeenLastCalledWith(expect.closeTo(0.54, 6));
    fireEvent.pointerUp(window, {});
  });

  it('locks a horizontal drag to its initial axis and ignores small clicks', () => {
    const k = knob();
    fireEvent.pointerDown(k.dial, { button: 0, clientX: 100, clientY: 100 });
    fireEvent.pointerMove(window, { clientX: 103, clientY: 100 });
    expect(k.onChange).not.toHaveBeenCalled();
    fireEvent.pointerMove(window, { clientX: 130, clientY: 102 });
    expect(k.onChange).toHaveBeenLastCalledWith(expect.closeTo(0.7, 6));
    fireEvent.pointerMove(window, { clientX: 130, clientY: 70 });
    expect(k.onChange).toHaveBeenCalledOnce();
    fireEvent.pointerMove(window, { clientX: 115, clientY: 70, shiftKey: true });
    expect(k.onChange).toHaveBeenLastCalledWith(expect.closeTo(0.68, 6));
    fireEvent.pointerUp(window, {});
  });

  it('stops listening when the drag ends, and releases the drag cursor', () => {
    const k = knob();
    fireEvent.pointerDown(k.dial, { button: 0, clientY: 200 });
    expect(document.documentElement.classList.contains('dragcur')).toBe(true);
    fireEvent.pointerUp(window, {});
    expect(document.documentElement.classList.contains('dragcur')).toBe(false);
    fireEvent.pointerMove(window, { clientY: 100 });
    expect(k.onChange).not.toHaveBeenCalled();
  });

  it('a cancelled pointer ends the drag too', () => {
    const k = knob();
    fireEvent.pointerDown(k.dial, { button: 0, clientY: 200 });
    fireEvent.pointerCancel(window, {});
    expect(k.onEnd).toHaveBeenCalledOnce();
    fireEvent.pointerMove(window, { clientY: 100 });
    expect(k.onChange).not.toHaveBeenCalled();
  });

  it('Alt+click or a double click resets to the default instead of dragging', () => {
    const k = knob();
    fireEvent.pointerDown(k.dial, { button: 0, altKey: true, clientX: 5, clientY: 5 });
    expect(k.onReset).toHaveBeenCalledOnce();
    expect(k.onStart).not.toHaveBeenCalled();
    resetDblCheck();
    fireEvent.pointerDown(k.dial, { button: 0, clientX: 5, clientY: 5 });
    fireEvent.pointerUp(window, {});
    fireEvent.pointerDown(k.dial, { button: 0, clientX: 5, clientY: 5 });
    expect(k.onReset).toHaveBeenCalledTimes(2);
  });

  it('right-click opens its menu at the pointer and does not drag', () => {
    const k = knob();
    fireEvent.pointerDown(k.dial, { button: 2, clientX: 40, clientY: 60 });
    expect(k.onMenu).toHaveBeenCalledWith(40, 60);
    expect(k.onStart).not.toHaveBeenCalled();
  });

  it('the wheel steps 2 % (Shift: 0.4 %) and ignores a zero delta', () => {
    const k = knob();
    fireEvent.wheel(k.dial, { deltaY: -100 });
    expect(k.onWheel).toHaveBeenLastCalledWith(expect.closeTo(0.52, 6));
    fireEvent.wheel(k.dial, { deltaY: 100, shiftKey: true });
    expect(k.onWheel).toHaveBeenLastCalledWith(expect.closeTo(0.496, 6));
    fireEvent.wheel(k.dial, { deltaY: 0 });
    expect(k.onWheel).toHaveBeenCalledTimes(2);
  });

  it('marks an automated parameter', () => {
    const [auto, setAuto] = createSignal(false);
    const k = knob({ automated: auto });
    const root = k.dial.parentElement!;
    expect(root.classList.contains('auto')).toBe(false);
    setAuto(true);
    settle();
    expect(root.classList.contains('auto')).toBe(true);
    expect(root.querySelector('.autodot')).not.toBeNull();
  });
});

describe('MiniKnob', () => {
  it('drags (120 px travel), resets on Alt, opens its menu on right-click, never lets a click through', () => {
    const [v, setV] = createSignal(0);
    const h = { onStart: vi.fn(), onChange: vi.fn((t: number) => setV(t)), onEnd: vi.fn(), onReset: vi.fn(), onWheel: vi.fn(), onMenu: vi.fn() };
    const outer = vi.fn();
    render(() => <div onClick={outer}><MiniKnob t={v} key="k" title="Send level" {...h} /></div>);
    const el = screen.getByTitle('Send level');
    fireEvent.pointerDown(el, { button: 0, clientY: 100 });
    fireEvent.pointerMove(window, { clientY: 40 });
    fireEvent.pointerUp(window, {});
    expect(h.onChange).toHaveBeenLastCalledWith(0.5);
    expect(h.onEnd).toHaveBeenCalledOnce();
    fireEvent.pointerDown(el, { button: 0, altKey: true });
    expect(h.onReset).toHaveBeenCalledOnce();
    fireEvent.pointerDown(el, { button: 2, clientX: 3, clientY: 4 });
    expect(h.onMenu).toHaveBeenCalledWith(3, 4);
    fireEvent.click(el);
    expect(outer).not.toHaveBeenCalled();
  });
});

describe('Segmented', () => {
  it('lights the current option and picks another on click', () => {
    const [v, setV] = createSignal(1);
    const pick = vi.fn((i: number) => setV(i));
    render(() => <Segmented options={['Off', 'Soft', 'Hard']} value={v} pick={pick} />);
    expect(screen.getByRole('button', { name: 'Soft' }).classList.contains('on')).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'Hard' }));
    settle();
    expect(pick).toHaveBeenCalledWith(2);
    expect(screen.getByRole('button', { name: 'Hard' }).classList.contains('on')).toBe(true);
    expect(screen.getByRole('button', { name: 'Soft' }).classList.contains('on')).toBe(false);
  });
});

describe('NumField', () => {
  function field(disabled = false) {
    const [v, setV] = createSignal(440);
    const apply = vi.fn((x: number, _mode: string) => { setV(x); });
    const r = render(() => (
      <NumField label="Freq" value={v} text={x => x + ' Hz'} drag={(v0, d) => v0 + d} parse={parseNum} apply={apply} disabled={() => disabled} />
    ));
    return { apply, el: r.container.querySelector('.numf') as HTMLElement };
  }

  it('drags only after 2 px, then begins, moves and ends', () => {
    const { apply, el } = field();
    fireEvent.pointerDown(el, { button: 0, clientX: 0, clientY: 100 });
    fireEvent.pointerMove(window, { clientX: 1, clientY: 100 });
    expect(apply).not.toHaveBeenCalled();
    fireEvent.pointerMove(window, { clientX: 0, clientY: 90 });
    expect(apply.mock.calls.map(c => c[1])).toEqual(['begin', 'move']);
    expect(apply).toHaveBeenLastCalledWith(450, 'move');
    fireEvent.pointerUp(window, {});
    expect(apply).toHaveBeenLastCalledWith(450, 'end');
  });

  it('a press without a drag is no edit', () => {
    const { apply, el } = field();
    fireEvent.pointerDown(el, { button: 0, clientX: 0, clientY: 0 });
    fireEvent.pointerUp(window, {});
    expect(apply).not.toHaveBeenCalled();
  });

  it('double-click to type: Enter sets the parsed value, Escape keeps the old one', async () => {
    const { apply, el } = field();
    const dbl = () => { fireEvent.pointerDown(el, { button: 0 }); fireEvent.pointerUp(window, {}); fireEvent.pointerDown(el, { button: 0 }); };
    dbl();
    const input = await screen.findByRole('textbox');
    (input as HTMLInputElement).value = '1.2k';
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(apply).toHaveBeenLastCalledWith(1200, 'set');
    expect(screen.queryByRole('textbox')).toBeNull();
    resetDblCheck();
    dbl();
    const again = await screen.findByRole('textbox');
    (again as HTMLInputElement).value = '9';
    fireEvent.keyDown(again, { key: 'Escape' });
    expect(apply).toHaveBeenCalledTimes(1);
    expect(screen.getByText('1200 Hz')).toBeTruthy();
  });

  it('disabled: shows a dash and ignores drags and the wheel', () => {
    const { apply, el } = field(true);
    expect(el.textContent).toContain('—');
    fireEvent.pointerDown(el, { button: 0, clientY: 100 });
    fireEvent.pointerMove(window, { clientY: 0 });
    fireEvent.pointerUp(window, {});
    fireEvent.wheel(el, { deltaY: -100 });
    expect(apply).not.toHaveBeenCalled();
  });

  it('parseNum reads k suffixes, commas and units', () => {
    expect(parseNum('1.2k')).toBe(1200);
    expect(parseNum('-3,5 dB')).toBe(-3.5);
    expect(parseNum('abc')).toBeNull();
  });
});
