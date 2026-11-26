// The M / S pair on Playlist tracks and Mixer strips (FL gestures).
import { fireEvent, render, screen } from '@solidjs/testing-library';
import { createSignal } from '../lib/reactive';
import { describe, expect, it, vi } from 'vitest';
import { settle } from '../test/settle';
import { MuteSolo } from './MuteSolo';

function pair(noSolo = false) {
  const [muted, setMuted] = createSignal(false), [solo, setSolo] = createSignal(false);
  const onMute = vi.fn(() => setMuted(m => !m)), onSolo = vi.fn(() => setSolo(s => !s));
  const outer = vi.fn();
  render(() => (
    <div onPointerDown={outer}>
      <MuteSolo muted={muted()} solo={solo()} muteTip="Mute" soloTip="Solo" onMute={onMute} onSolo={onSolo} noSolo={noSolo} />
    </div>
  ));
  return { onMute, onSolo, outer, m: screen.getByTitle('Mute'), s: () => screen.queryByTitle('Solo') };
}

describe('MuteSolo', () => {
  it('click M mutes and lights it; click S solos and lights it', () => {
    const p = pair();
    fireEvent.click(p.m);
    settle();
    expect(p.onMute).toHaveBeenCalledOnce();
    expect(p.m.classList.contains('on')).toBe(true);
    fireEvent.click(p.s()!);
    settle();
    expect(p.onSolo).toHaveBeenCalledWith(false);
    expect(p.s()!.classList.contains('on')).toBe(true);
  });

  it('Ctrl+click S adds to the soloed ones', () => {
    const p = pair();
    fireEvent.click(p.s()!, { ctrlKey: true });
    expect(p.onSolo).toHaveBeenCalledWith(true);
  });

  it('Ctrl, Alt or right-click on M solos instead of muting', () => {
    const p = pair();
    fireEvent.click(p.m, { ctrlKey: true });
    fireEvent.click(p.m, { altKey: true });
    fireEvent.pointerDown(p.m, { button: 2 });
    expect(p.onSolo).toHaveBeenCalledTimes(3);
    expect(p.onMute).not.toHaveBeenCalled();
  });

  it('Ctrl+right-click on M adds to the soloed ones (a Playlist track has no S to Ctrl+click)', () => {
    const p = pair();
    fireEvent.pointerDown(p.m, { button: 2, ctrlKey: true });
    expect(p.onSolo).toHaveBeenCalledWith(true);
  });

  it('presses stay on the buttons (the track row under them does not start a drag)', () => {
    const p = pair();
    fireEvent.pointerDown(p.m, { button: 0 });
    fireEvent.pointerDown(p.s()!, { button: 0 });
    expect(p.outer).not.toHaveBeenCalled();
  });

  it('the master has no S', () => {
    const p = pair(true);
    expect(p.s()).toBeNull();
    expect(p.m).toBeTruthy();
  });
});
