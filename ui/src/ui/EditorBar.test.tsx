// The grid chip (Snap in the Piano roll and the Playlist).
import { fireEvent, render, screen } from '@solidjs/testing-library';
import { describe, expect, it, vi } from 'vitest';
import { createSignal } from '../lib/reactive';
import { settle } from '../test/settle';
import { GridChip } from './EditorBar';

function snap(initial = '1') {
  const [v, setV] = createSignal(initial);
  const onChange = vi.fn((x: string) => setV(x));
  render(() => (
    <GridChip label="Snap" value={v()} onChange={onChange}
      modes={[{ label: 'auto', value: '-1', note: 'now 1/32' }]}
      sections={[{ options: [['1/4', '4'], ['1/8', '2'], ['1/16', '1']] }, { label: 'Triplets', options: [['1/12', String(4 / 3)]] }]}
      implied={v() === '-1' ? '2' : undefined} />
  ));
  const chip = () => screen.getByRole('button', { name: /^Snap/ });
  const cell = (name: string) => screen.getByRole('radio', { name: new RegExp('^' + name) });
  return { onChange, chip, cell };
}

describe('GridChip', () => {
  it('shows the chosen value on the chip and opens every choice as a grid', () => {
    const s = snap();
    expect(s.chip().textContent).toContain('1/16');
    expect(screen.queryByRole('radiogroup')).toBeNull();
    fireEvent.click(s.chip());
    settle();
    expect(screen.getAllByRole('radio')).toHaveLength(5);
    expect(s.cell('1/16').getAttribute('aria-checked')).toBe('true');
    expect(screen.getByText('Triplets')).toBeTruthy();
  });

  it('a pick changes the value once and closes the grid; picking the current value changes nothing', () => {
    const s = snap();
    fireEvent.click(s.chip()); settle();
    fireEvent.click(s.cell('1/8')); settle();
    expect(s.onChange).toHaveBeenCalledWith('2', s.chip());
    expect(screen.queryByRole('radiogroup')).toBeNull();
    fireEvent.click(s.chip()); settle();
    fireEvent.click(s.cell('1/8')); settle();
    expect(s.onChange).toHaveBeenCalledOnce();
  });

  it('auto outlines the grid value it resolves to, with its note', () => {
    const s = snap('-1');
    expect(s.chip().textContent).toContain('auto');
    fireEvent.click(s.chip()); settle();
    expect(s.cell('auto').textContent).toContain('now 1/32');
    expect(s.cell('1/8').classList.contains('implied')).toBe(true);
    expect(s.cell('1/16').classList.contains('implied')).toBe(false);
  });

  it('Esc closes without a change and keeps the key from the editor behind', () => {
    const s = snap();
    const outer = vi.fn();
    document.body.addEventListener('keydown', outer);
    fireEvent.click(s.chip()); settle();
    fireEvent.keyDown(s.cell('1/16'), { key: 'Escape' }); settle();
    expect(screen.queryByRole('radiogroup')).toBeNull();
    expect(s.onChange).not.toHaveBeenCalled();
    expect(outer).not.toHaveBeenCalled();
    document.body.removeEventListener('keydown', outer);
  });

  it('a press outside closes it', () => {
    const s = snap();
    fireEvent.click(s.chip()); settle();
    fireEvent.pointerDown(document.body); settle();
    expect(screen.queryByRole('radiogroup')).toBeNull();
  });
});
