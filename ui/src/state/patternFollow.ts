// A pattern got longer or shorter (#245): the Playlist clips that show it followed, or stopped at
// the next clip on their track. The edit is usually made in the Channel Rack or the Piano roll,
// which can cover the Playlist, so it is said in a notice with Undo, and the clips flash briefly.
import { createRoot } from 'solid-js';
import type { Id } from '../api/types';
import { t } from '../lib/i18n';
import { barsText, BAR, type FollowReport } from '../lib/pattern';
import { createSignal, watch } from '../lib/reactive';
import { undo } from './actions';
import { dismissNotice, notice, notify, type Notice } from './notice';
import { onArrangementChange, onPatternFollow, S } from './store';

/** How long resized clips stay highlighted on the Playlist (ms). */
export const FOLLOW_FLASH_MS = 1400;

/** The clips a pattern-length change resized, and when (`performance.now()`). */
export interface FollowFlash { ids: ReadonlySet<Id>; stopped: ReadonlySet<Id>; at: number }
const [followFlash, setFollowFlash] = createSignal<FollowFlash | null>(null);
export { followFlash };

/** The notice's text for one edit's reports (one per pattern whose clips changed). */
export function followMessage(reports: readonly FollowReport[], name: (id: Id) => string): string {
  return reports.map(r => {
    const n = r.followed.length + r.stopped.length;
    const head = t('d.follow.resized', { name: name(r.pattern), bars: barsText(r.len / BAR), n });
    return r.stopped.length ? head + ' · ' + t('d.follow.stopped', { n: r.stopped.length }) : head;
  }).join(' · ');
}

createRoot(() => {
  let shown: Notice | null = null, lens = new Map<Id, number>();
  const close = () => { if (shown && notice() === shown) dismissNotice(); shown = null; };
  onPatternFollow(reports => {
    const stopped = reports.flatMap(r => r.stopped);
    setFollowFlash({ ids: new Set([...reports.flatMap(r => r.followed), ...stopped]), stopped: new Set(stopped), at: performance.now() });
    const name = (id: Id) => S.project.patterns.find(x => x.id === id)?.name ?? '?';
    notify(followMessage(reports, name), [{ label: t('a.mn.undo'), act: () => void undo() }]);
    shown = notice();
    lens = new Map(reports.map(r => [r.pattern, r.len]));
  });
  // Undo on the notice undoes the edit it describes, so the notice goes once that edit is no longer
  // the last one: another local edit (a drag of the same pattern's notes is still that edit), or a
  // state from the engine in which the pattern has another length (undone with Ctrl+Z, the CLI)
  onArrangementChange(ops => {
    if (!shown || !ops) return;
    const same = ops.every(op => (op.op === 'putPattern' && lens.has(op.pattern.id)) || (op.op === 'transformNotes' && lens.has(op.pattern)));
    if (!same) close();
  });
  watch(() => S.revision, () => {
    if (!shown) return;
    if (notice() !== shown) { shown = null; return; }
    for (const [id, len] of lens) if (S.project.patterns.find(x => x.id === id)?.len !== len) return close();
  }, { defer: true });
});
