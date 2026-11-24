// The song's key: one for the whole project (the toolbar shows it, every Piano roll highlights it,
// scale-aware transforms use it). Derived from the project every time, so undo and loading another
// project follow.
import { S } from './store';
import { NOTE_NAMES } from '../lib/pitch';
import { SCALES } from '../lib/pattern';
import { t } from '../lib/i18n';

/** Where the key comes from: chosen (`set`), detected by a Pitch Correct in the project (`tune`), or not known (`none`). */
export type KeySource = 'set' | 'tune' | 'none';
export interface SongKey { root: number; scale: number; from: KeySource }

/**
 * The key to highlight: the project key, else the project's Pitch Correct (a detected key), else
 * C in the project scale (what the engine's melody generator falls back to) — marked `none`, so a
 * view can say no key is known instead of pretending C major was chosen.
 * `scale` is the engine's scale id (pitch::scale_mask), not the list position.
 */
export function songKey(): SongKey {
  if (S.project.key != null) return { root: S.project.key, scale: S.project.scale ?? 1, from: 'set' };
  for (const m of S.project.inserts) for (const f of m.fx) {
    if (f.type === 'tune' && (f.params.scale ?? 0) > 0) return { root: Math.round(f.params.key ?? 0), scale: f.params.scale ?? 1, from: 'tune' };
  }
  return { root: 0, scale: S.project.scale ?? 1, from: 'none' };
}

/** "A minor" (the scale's name lower-cased after the note, as the Piano roll writes it). */
export function keyName(k: Pick<SongKey, 'root' | 'scale'>): string {
  const sc = SCALES.find(x => x[1] === k.scale);
  return NOTE_NAMES[((k.root % 12) + 12) % 12] + (sc ? ' ' + t(sc[0]).toLowerCase() : '');
}

export const setKeyOp = (key: number | null, scale: number) => ({ op: 'setKey' as const, key, scale });
