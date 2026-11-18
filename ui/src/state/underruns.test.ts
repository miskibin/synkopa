import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { prefs } from './prefs';

// prefs pulls in the app's actions (and through them the windowed UI): not needed for its defaults
vi.mock('./actions', () => ({}));
vi.mock('./ui', () => ({ U: {} }));
import { _resetUnderrunState, cpuBand, cpuText, FLASH_MS, noteUnderruns, resetUnderruns, underrunFlash, underruns } from './underruns';

describe('output underrun counter', () => {
  beforeEach(() => { vi.useFakeTimers(); _resetUnderrunState(); });
  afterEach(() => vi.useRealTimers());

  it('shows what the engine counted before the window opened, without flashing', () => {
    noteUnderruns(3);
    expect(underruns()).toBe(3);
    expect(underrunFlash()).toBe(false);
  });

  it('flashes briefly on each new underrun and keeps the count', () => {
    noteUnderruns(0);
    noteUnderruns(0);
    expect(underrunFlash()).toBe(false);
    noteUnderruns(1);
    expect(underruns()).toBe(1);
    expect(underrunFlash()).toBe(true);
    vi.advanceTimersByTime(FLASH_MS - 1);
    noteUnderruns(2); // another one extends the flash
    vi.advanceTimersByTime(FLASH_MS - 1);
    expect(underrunFlash()).toBe(true);
    vi.advanceTimersByTime(2);
    expect(underrunFlash()).toBe(false);
    expect(underruns()).toBe(2);
  });

  it('counts from the reset on, without a round trip to the engine', () => {
    noteUnderruns(5);
    resetUnderruns();
    expect(underruns()).toBe(0);
    noteUnderruns(5);
    expect(underruns()).toBe(0);
    noteUnderruns(7);
    expect(underruns()).toBe(2);
  });

  it('starts over when a fresh engine counts from zero again', () => {
    noteUnderruns(9);
    resetUnderruns();
    noteUnderruns(1);
    expect(underruns()).toBe(1);
  });
});

describe('cpu meter', () => {
  it('is off in the toolbar until turned on in Settings → Audio (the bar is short on room)', () => {
    expect(prefs().cpuMeter).toBe(false);
  });
  it('turns amber from 60 % and red from 85 %, where the output starts to crackle', () => {
    expect(cpuBand(0)).toBe('ok');
    expect(cpuBand(0.59)).toBe('ok');
    expect(cpuBand(0.6)).toBe('warn');
    expect(cpuBand(0.85)).toBe('hot');
    expect(cpuBand(1.4)).toBe('hot');
  });
  it('prints whole percent within a fixed width', () => {
    expect(cpuText(0.123)).toBe('12%');
    expect(cpuText(0)).toBe('0%');
    expect(cpuText(NaN)).toBe('0%');
    expect(cpuText(1.5)).toBe('150%');
    expect(cpuText(40)).toBe('999%');
  });
});
