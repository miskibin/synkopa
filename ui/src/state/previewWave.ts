// What the Browser is previewing, for the toolbar's status line (FL shows the clicked sample there):
// its name, length and waveform, keyed like `U.previewPath` (a file path, or `src:<id>` for a project
// source). The status line shows it while that preview plays, and the hint text otherwise.
import { createSignal } from '../lib/reactive';
import type { PreviewWave } from '../api/types';
import { U } from './ui';

export interface PreviewShown { key: string; name: string; secs: number; peaks: Float32Array }

const [wave, setWave] = createSignal<PreviewShown | null>(null);

/** Remember what preview `key` looks like; a late answer for an earlier preview is dropped. */
export function setPreviewWave(key: string, name: string, w: PreviewWave) {
  if (U.previewPath !== key) return;
  setWave({ key, name, secs: w.secs, peaks: Float32Array.from(w.peaks) });
}

/** The preview the status line shows now: the one that is playing, if its waveform is known. */
export function shownPreview(): PreviewShown | null {
  const w = wave();
  return w && U.previewing && U.previewPath === w.key ? w : null;
}
