// Chords of an audio clip (the tempo and key dialog's Chords section): the model's install state and its one
// download, the detail the musician picked (kept between uses), and "Send to piano roll".
import { createStore } from '../lib/reactive';
import { be } from '../api/backend';
import type { ChordDetail, ChordModelStatus, Id } from '../api/types';
import { t } from '../lib/i18n';
import { endWheelGestures, enqueue, resync } from './edit';
import { openRoll, setCurPattern } from './pattern';
import { say, setU } from './ui';

interface ChordsState {
  model: ChordModelStatus | null;
  /** 0..1 while the model downloads. */
  download: number | null;
  /** Why the download failed. */
  error: string | null;
  detail: ChordDetail;
}

const [st, setSt] = createStore<ChordsState>({ model: null, download: null, error: null, detail: 'full' });
export const CHORDS = st;
export const setChordDetail = (d: ChordDetail) => setSt('detail', d);

export function refreshChordModel() {
  be().chordsModelStatus().then(m => setSt('model', m)).catch(() => {});
}

/** Downloads the model (once; a second call while it runs does nothing). Resolves true when it is installed. */
export async function installChordModel(): Promise<boolean> {
  if (st.download != null) return false;
  setSt({ download: 0, error: null });
  const off = be().on('model-progress', p => { if (p.id === 'chordmini' && p.total > 0) setSt('download', p.done / p.total); });
  try {
    setSt('model', await be().chordsModelInstall());
    return true;
  } catch (e) {
    setSt('error', String((e as Error)?.message ?? e));
    return false;
  } finally {
    setSt('download', null);
    void off.then(f => f());
  }
}

/**
 * "Send to piano roll": the chords between `from..to` as a new pattern (on `channel`, else a new piano), shown in
 * the Piano roll. One undo step. Resolves false (with the reason said) when nothing was written.
 */
export async function sendChordsToRoll(a: { clipId: Id; from: number; to: number; detail: ChordDetail; channel: Id | null }): Promise<boolean> {
  endWheelGestures();
  try {
    const r = await enqueue(() => be().chordsToPattern(a));
    await resync();
    setCurPattern(r.patternId, true);
    setU('musicWin', null);
    openRoll(r.channelId);
    say(t(r.newChannel ? 'd.chords.say.doneNew' : 'd.chords.say.done', { n: r.chords }));
    return true;
  } catch (e) {
    say(t('d.chords.say.fail', { err: String((e as Error)?.message ?? e) }));
    void resync();
    return false;
  }
}
