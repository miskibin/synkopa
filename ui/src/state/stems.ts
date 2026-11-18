// "Extract stems" (FL's *Extract stems from sample*): the dialog's open clip, the one job that may run
// (model download, then the separation, in the background while the musician keeps working) and its
// last error. The job is not queued behind edits (`enqueue`): it takes minutes and edits must go on.
import { createStore } from 'solid-js/store';
import { be } from '../api/backend';
import { ERR_NO_STEM_MODEL } from '../api/errors';
import type { Id, StemId, StemModelStatus, StemsProgress } from '../api/types';
import { t } from '../lib/i18n';
import { endWheelGestures, enqueue, resync } from './edit';
import { S } from './store';
import { say, setU } from './ui';

export const STEM_IDS: StemId[] = ['drums', 'bass', 'other', 'vocals'];
/** The colours the engine gives each stem's track (app/src/stems_actions.rs `STEMS`). */
export const STEM_COLORS: Record<StemId, string> = { drums: '#e8a33d', bass: '#8f6ee8', other: '#47b8a8', vocals: '#e05a8a' };

export type StemsStage = 'download' | StemsProgress['stage'];
export interface StemsJob { clipId: Id; name: string; stems: StemId[]; stage: StemsStage; frac: number }

interface StemsState {
  /** The clip whose dialog is open (`null` = closed; a running job then shows as a chip). */
  win: Id | null;
  job: StemsJob | null;
  /** Why the last job for `errorClip` failed (shown in its dialog). */
  error: string | null;
  errorClip: Id | null;
  model: StemModelStatus | null;
  /** The stems ticked in the dialog (kept between uses, like FL). */
  picked: StemId[];
}

const [st, setSt] = createStore<StemsState>({ win: null, job: null, error: null, errorClip: null, model: null, picked: [...STEM_IDS] });
export const STEMS = st;

export function refreshStemModel() {
  be().stemsModelStatus().then(m => setSt('model', m)).catch(() => {});
}

export function openStems(clipId: Id) {
  setSt({ win: clipId, ...(st.errorClip !== clipId ? { error: null, errorClip: null } : {}) });
  refreshStemModel();
}
/** Hides the dialog; a running job goes on (its chip brings the dialog back). */
export const closeStems = () => setSt('win', null);
export const toggleStem = (s: StemId) => setSt('picked', p => (p.includes(s) ? p.filter(x => x !== s) : STEM_IDS.filter(x => x === s || p.includes(x))));
export const cancelStems = () => { void be().stemsCancel(); };

const sourceName = (clipId: Id) => {
  const c = S.project.clips.find(x => x.id === clipId);
  return S.project.sources.find(s => s.id === c?.ref)?.name ?? '';
};

/** Downloads the model when it is missing, then separates `clipId` into the ticked stems. */
export async function runStems(clipId: Id) {
  if (st.job) return;
  const stems = STEM_IDS.filter(s => st.picked.includes(s));
  if (!stems.length) return;
  const name = sourceName(clipId);
  const needModel = !st.model?.installed;
  setSt({ job: { clipId, name, stems, stage: needModel ? 'download' : 'prepare', frac: 0 }, error: null, errorClip: null });
  const offP = be().on('stems-progress', p => { if (st.job?.clipId === p.clipId) setSt('job', { stage: p.stage, frac: p.frac }); });
  const offM = be().on('model-progress', p => { if (p.id === 'htdemucs' && st.job?.stage === 'download' && p.total > 0) setSt('job', 'frac', p.done / p.total); });
  const fail = (msg: string) => {
    setSt({ error: msg, errorClip: clipId, win: clipId });
    say(t('d.stems.say.fail', { err: msg }));
  };
  try {
    if (needModel) {
      let m;
      try { m = await be().stemsModelInstall(); } catch (e) { fail(t('d.stems.err.download', { err: String((e as Error)?.message ?? e) })); return; }
      if ('cancelled' in m) { say(t('d.stems.say.cancelled')); return; }
      setSt('model', m);
      setSt('job', { stage: 'prepare', frac: 0 });
    }
    // the engine reads the clip as it is: edits made so far must have reached it
    endWheelGestures();
    await enqueue(async () => {});
    const r = await be().extractStems(clipId, stems);
    if ('cancelled' in r) { say(t('d.stems.say.cancelled')); return; }
    await enqueue(async () => {});
    await resync();
    setU('sel', r.clipIds);
    if (st.win === clipId) setSt('win', null);
    say(r.clipIds.length === 1 ? t('d.stems.say.done1', { name }) : t('d.stems.say.done', { n: r.clipIds.length, name }));
  } catch (e) {
    const msg = String((e as Error)?.message ?? e);
    if (msg.startsWith(ERR_NO_STEM_MODEL)) refreshStemModel();
    fail(msg.replace(ERR_NO_STEM_MODEL, '').trim());
    void resync();
  } finally {
    setSt('job', null);
    void offP.then(f => f());
    void offM.then(f => f());
  }
}

/** A length of audio: "0:12", "3:24". */
export function fmtSpan(s: number): string {
  const all = Math.max(0, Math.round(s)), m = Math.floor(all / 60);
  return `${m}:${String(all % 60).padStart(2, '0')}`;
}
/** A wait: "20 s", "2 min". */
export const fmtEta = (s: number) => (s < 60 ? Math.max(5, Math.round(s / 5) * 5) + ' s' : Math.round(s / 60) + ' min');
/** Measured on a 6-core desktop CPU: ~0.38 s per second of audio plus the model load; slower machines take longer. */
export const stemsEta = (audioS: number) => 2 + (audioS + 6) * 0.4;
