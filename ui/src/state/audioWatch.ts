import { createSignal } from 'solid-js';
import { be } from '../api/backend';
import type { AudioConfig, AudioStatus } from '../api/types';
import { t } from '../lib/i18n';

/** What to tell the user when the audio status changes from `prev` to `next`; null = nothing new. */
export function audioNotice(prev: AudioStatus | null, next: AudioStatus): string | null {
  if (next.unavailableOutput) {
    return prev?.host === next.host && prev.unavailableOutput === next.unavailableOutput
      ? null : t('d.audio.unavailable', { device: next.unavailableOutput });
  }
  if (next.error && (!prev || prev.error !== next.error)) return t('d.audio.stopped', { err: next.error });
  if (prev && next.restarts > prev.restarts) return t('d.audio.restarted', { why: next.note ?? '' });
  return null;
}

const [audioStatus, setAudioStatus] = createSignal<AudioStatus | null>(null);
const [audioToast, setAudioToast] = createSignal<AudioStatus | null>(null);
const [audioBusy, setAudioBusy] = createSignal(false);
const [audioApplyError, setAudioApplyError] = createSignal<string | null>(null);
export { audioStatus, audioToast, audioBusy, audioApplyError };
export const dismissAudioToast = () => setAudioToast(null);

/** Subscribe before fetching the snapshot so startup or a hot unplug can't be missed. */
export async function watchAudio(
  initial: () => Promise<AudioStatus>,
  on: (cb: (s: AudioStatus) => void) => Promise<unknown>,
  say: (msg: string) => void,
) {
  let last: AudioStatus | null = null;
  const update = (s: AudioStatus) => {
    const msg = audioNotice(last, s);
    last = s;
    setAudioStatus(s);
    if (msg && s.unavailableOutput) setAudioToast(s);
    if (!s.unavailableOutput) {
      setAudioToast(current => s.host === 'wasapi' && s.error && current?.host === 'wasapi' ? s : null);
    }
    if (msg) say(msg);
  };
  await on(update);
  const snapshot = await initial();
  if (!last) update(snapshot);
}

export function defaultAudioConfig(host: string): AudioConfig {
  return { host, output: null, input: null, sampleRate: null, bufferSize: null, inputChannel: null };
}

export async function applyAudioConfig(cfg: AudioConfig, recovering = false) {
  if (audioBusy()) return;
  setAudioBusy(true);
  setAudioApplyError(null);
  try {
    const status = await be().audioSetConfig(cfg);
    if (recovering) setAudioToast(status.running ? null : status);
  } catch (e) {
    setAudioApplyError(String(e));
  } finally {
    setAudioBusy(false);
  }
}

export const switchToWasapi = () => applyAudioConfig(defaultAudioConfig('wasapi'), true);
