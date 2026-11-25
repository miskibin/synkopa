// Convert to MIDI (Playlist audio clip menu): the dialog's clip, its settings (kept between uses, like FL's
// dialogs) and the engine's preview of what Convert will write. Every preview and the convert itself go through
// `audio_to_midi`, so what the dialog shows is what lands in the pattern.
import { createStore } from '../lib/reactive';
import { be } from '../api/backend';
import type { AudioToMidiResult, Id, PitchAnalysis } from '../api/types';
import { t } from '../lib/i18n';
import { MELODY_DEFAULTS } from '../lib/audioMidi';
import { endWheelGestures, enqueue, resync } from './edit';
import { openRoll, setCurPattern } from './pattern';
import { S } from './store';
import { say } from './ui';

export interface AudioMidiOpts {
  /** Shortest note kept, seconds. */
  shortest: number;
  /** Grid in steps (1 = 1/16); 0 = as sung. */
  quantize: number;
  /** null = follow the singing. */
  velocity: number | null;
  /** Key range (null = no limit). */
  low: number | null;
  high: number | null;
  /** Target channel (null = a new piano). */
  channel: Id | null;
}

interface AudioMidiState {
  win: Id | null;
  opts: AudioMidiOpts;
  preview: AudioToMidiResult | null;
  /** The clip's pitch curve, for the preview's background. */
  curve: PitchAnalysis | null;
  loading: boolean;
  busy: boolean;
  error: string | null;
}

const [st, setSt] = createStore<AudioMidiState>({
  win: null,
  opts: { shortest: MELODY_DEFAULTS.shortest, quantize: 0, velocity: null, low: null, high: null, channel: null },
  preview: null, curve: null, loading: false, busy: false, error: null,
});
export const AMIDI = st;

let seq = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
/** The clip's pitch analysis, started once when the dialog opens. Previews wait for it, so a
 *  clip not analysed yet is analysed once, not once per preview (each a multi-second job). */
let analysed: Promise<unknown> = Promise.resolve();

const args = (clipId: Id) => ({ clipId, ...st.opts });

async function refresh() {
  const clipId = st.win; if (clipId == null) return;
  const my = ++seq;
  setSt({ loading: true });
  try {
    await analysed.catch(() => {});
    // settings changed while the analysis ran: only the latest preview is worth asking for
    if (my !== seq) return;
    const r = await be().audioToMidi({ ...args(clipId), preview: true });
    if (my === seq) setSt({ preview: r, error: null, loading: false });
  } catch (e) {
    if (my === seq) setSt({ preview: null, error: String((e as Error)?.message ?? e), loading: false });
  }
}

/** Opens the dialog for `clipId`; the key range starts open (the take's own range is shown as a hint). */
export function openAudioMidi(clipId: Id) {
  const c = S.project.clips.find(x => x.id === clipId);
  if (!c || c.kind !== 'audio') return;
  const keepChannel = st.opts.channel != null && (S.project.channels ?? []).some(ch => ch.id === st.opts.channel);
  setSt({ win: clipId, preview: null, curve: null, error: null, busy: false, opts: { ...st.opts, low: null, high: null, channel: keepChannel ? st.opts.channel : null } });
  analysed = be().pitchAnalyze(clipId).then(a => { if (st.win === clipId) setSt('curve', a); });
  void refresh();
}
export const closeAudioMidi = () => { seq++; setSt({ win: null, preview: null, curve: null }); };

/** Changes settings; the preview follows (at once for a click, after the drag pauses for a drag). */
export function setAudioMidiOpts(p: Partial<AudioMidiOpts>, settle = false) {
  setSt('opts', o => ({ ...o, ...p }));
  clearTimeout(timer);
  if (settle) void refresh(); else timer = setTimeout(() => void refresh(), 120);
}

/** Writes the notes: a new pattern under the clip, shown in the Piano roll. One undo step. */
export async function convertAudioMidi(): Promise<boolean> {
  const clipId = st.win; if (clipId == null || st.busy) return false;
  endWheelGestures();
  setSt('busy', true);
  try {
    const r = await enqueue(() => be().audioToMidi(args(clipId)));
    await resync();
    if (r.patternId != null) setCurPattern(r.patternId, true);
    closeAudioMidi();
    if (r.channelId != null) openRoll(r.channelId);
    say(t(r.newChannel ? 'd.a2m.say.doneNew' : 'd.a2m.say.done', { n: r.count }));
    return true;
  } catch (e) {
    const msg = String((e as Error)?.message ?? e);
    setSt({ busy: false, error: msg });
    say(t('d.a2m.say.fail', { err: msg }));
    void resync();
    return false;
  }
}
