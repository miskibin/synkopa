import { createSignal } from '../lib/reactive';
import { be } from '../api/backend';
import { VOL_UNITY } from '../api/types';

/**
 * FL Studio's main volume (the toolbar knob): how loud the app plays — the song, pattern and sample
 * previews, the metronome — independent of the project's Master fader. A per-user setting, not
 * part of the project, not undone, not automated and not applied to exports.
 */
const [vol, setVol] = createSignal(VOL_UNITY);
export const programVolume = vol;

/** How long after the last change the value is written to the settings file. */
const SAVE_MS = 400;
let loaded = false;
let saveTimer: ReturnType<typeof setTimeout> | undefined;
/** Bumped on every local change: a load that answers after one does not overwrite it. */
let gen = 0;

/** Reads the saved main volume once (the engine already plays at it). */
export function loadProgramVolume() {
  if (loaded) return; loaded = true;
  const g = gen;
  be().programVolume().then(v => { if (g === gen && Number.isFinite(v)) setVol(v); }).catch(() => {});
}

/** Sets the main volume at once (a fader position, `VOL_UNITY` = 0 dB); saved shortly after it settles. */
export function setProgramVolume(v: number) {
  const c = Math.min(1, Math.max(0, Number.isFinite(v) ? v : VOL_UNITY));
  gen++;
  setVol(c);
  void be().setProgramVolume(c, false).catch(() => {});
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => { void be().setProgramVolume(vol(), true).catch(() => {}); }, SAVE_MS);
}
