// The Playlist's own editing tools and snap, in its header row beside the overview (as FL Studio's
// Playlist carries its tools): they act on the Playlist only, so they live with it, not in the app bar.
import { For } from 'solid-js';
import { t } from '../lib/i18n';
import { autoSnap, SNAPS, snapName } from '../lib/snap';
import * as A from '../state/actions';
import { setU, U } from '../state/ui';
import { Icon, ICONS } from '../ui/icons';
import { TOOL_HINT, TOOLS } from './Toolbar';

export function PlaylistTools() {
  return (
    <div class="pltools" style={{ 'min-width': U.headW + 'px' }} onPointerDown={e => e.stopPropagation()}>
      <div class="seg tools" role="radiogroup" aria-label={t('a.pl.toolsLabel')}>
        <For each={TOOLS}>
          {([id, key, title]) => (
            <button role="radio" aria-checked={U.tool === id} classList={{ on: U.tool === id }} title={t(title) + ' (' + key + ') — ' + t(TOOL_HINT[id])} onClick={() => setU('tool', id)}>
              <Icon d={ICONS[id]} sw={1.9} size={13} />
            </button>
          )}
        </For>
      </div>
      {/* the select names the setting only ("Snap: auto"); what auto resolves to is in its tooltip and the status line */}
      <select class="sel snapsel" title={U.snap < 0 ? t('a.tb.snapTipAuto', { unit: snapName(autoSnap(U.bw)) }) : t('a.tb.snapTip')} value={String(U.snap)} onChange={e => A.setSnap(parseFloat(e.currentTarget.value))} data-snap>
        <For each={SNAPS}>{([label, v]) => <option value={String(v)}>{v === 0 ? t('a.snap.off') : label}</option>}</For>
      </select>
    </div>
  );
}
