import { createSignal } from 'solid-js';
import { be } from '../api/backend';
import type { AudioConfig, AudioStatus } from '../api/types';
import { t } from '../lib/i18n';
import { notify } from './notice';
import { setU } from './ui';

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
/** Reads the status again: the engine reports device changes by itself, but not an input it
 * opened for recording or its dropout count. */
export const refreshAudioStatus = () => be().audioGetConfig().then(setAudioStatus).catch(() => {});
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

/** Why nothing can be heard (the output failed to open), or null when audio runs or is not known yet. */
export function noAudioReason(s: AudioStatus | null): string | null {
  if (!s || s.running) return null;
  return s.error ?? s.unavailableOutput ?? null;
}

const [audioTabWanted, setAudioTabWanted] = createSignal(false);
/** "Audio settings" on a notice asked for the Audio tab (a signal: Settings may already be open). */
export const audioTabRequested = audioTabWanted;
/** Settings shows the Audio tab once per request (after "Audio settings" on a notice). */
export const takeAudioTabRequest = () => { const w = audioTabWanted(); if (w) setAudioTabWanted(false); return w; };
export function openAudioSettings() { setAudioTabWanted(true); setU('settings', true); }

/**
 * Play / pattern play while the output is not open: transport would start but nothing would
 * move or sound, so say why and offer a retry (the device may be back) and the audio settings.
 * Returns true when it warned.
 */
export function warnIfNoAudio(): boolean {
  const s = audioStatus();
  const why = noAudioReason(s);
  if (!s || !why) return false;
  const retry = async () => {
    await applyAudioConfig({ host: s.host, output: s.output, input: s.input, sampleRate: s.sampleRate, bufferSize: s.bufferSize, inputChannel: s.inputChannel ?? null });
    const now = audioStatus();
    const still = noAudioReason(now);
    if (still) notify(t('d.audio.play.err', { err: still }), [{ label: t('d.audio.play.settings'), act: openAudioSettings }], 12000);
    else notify(t('d.audio.play.back'));
  };
  notify(t('d.audio.play.err', { err: why }), [
    { label: t('d.audio.play.retry'), act: () => void retry() },
    { label: t('d.audio.play.settings'), act: openAudioSettings },
  ], 12000);
  return true;
}
