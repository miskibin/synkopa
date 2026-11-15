// Hot reload of the project's audio files (`media-reload`, app/src/media_watch.rs): the engine swaps the audio in
// by itself and the waveforms follow `audioGen`; this only tells the musician what happened, in the status line.
import type { MediaReload } from '../api/types';
import { t } from '../lib/i18n';

/** Events closer together than this are one notice (a folder of stems re-bounced at once). */
export const MEDIA_NOTICE_MS = 400;

const secs = (s: number) => String(+s.toFixed(s < 10 ? 2 : 1));

/** The status-line text for events that arrived together (oldest first). */
export function mediaNotice(evs: MediaReload[]): string | null {
  if (!evs.length) return null;
  // a failure or a vanished file matters more than a reload that worked
  const bad = evs.filter(e => e.kind !== 'reloaded');
  if (bad.length) {
    const e = bad[bad.length - 1];
    const one = e.kind === 'gone' ? t('d.media.gone', { name: e.name }) : t('d.media.failed', { name: e.name, err: e.error ?? '' });
    return bad.length > 1 ? `${one} ${t('d.media.more', { n: bad.length - 1 })}` : one;
  }
  const names = new Set(evs.map(e => e.path));
  if (names.size > 1) return t('d.media.many', { n: names.size });
  const e = evs[evs.length - 1];
  if (e.wasMissing) return t('d.media.back', { name: e.name });
  if (e.lengthDiffers && e.seconds != null) return t('d.media.length', { name: e.name, secs: secs(e.seconds) });
  return t('d.media.reloaded', { name: e.name });
}

/** Listens for `media-reload` and says what happened (coalesced). Returns the unsubscribe. */
export async function watchMedia(
  on: (cb: (e: MediaReload) => void) => Promise<() => void>,
  say: (msg: string) => void,
  wait = MEDIA_NOTICE_MS,
) {
  let held: MediaReload[] = [];
  let timer: ReturnType<typeof setTimeout> | null = null;
  return on(e => {
    held.push(e);
    if (timer) return;
    timer = setTimeout(() => {
      const msg = mediaNotice(held);
      held = []; timer = null;
      if (msg) say(msg);
    }, wait);
  });
}
