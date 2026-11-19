// The Playlist's own bar: the edit tools and Snap. They act on the Playlist only, so they sit in
// its header (over the track names) instead of the app bar.
import { t } from '../lib/i18n';
import { autoSnap, SNAPS, snapName } from '../lib/snap';
import * as A from '../state/actions';
import { setU, U } from '../state/ui';
import { ChipSelect, ToolSeg } from '../ui/EditorBar';
import { ICONS } from '../ui/icons';
import { TOOL_HINT, TOOLS } from './Toolbar';

export function PlaylistTools() {
  return (
    <div class="pltools" style={{ 'min-width': U.headW + 'px' }} onPointerDown={e => e.stopPropagation()}>
      <ToolSeg label={t('a.pl.toolsLabel')} size={13} sw={1.9} value={U.tool} onPick={id => setU('tool', id)}
        tools={TOOLS.map(([id, key, title]) => ({ id, icon: ICONS[id], title: t(title) + ' (' + key + ') — ' + t(TOOL_HINT[id]) }))} />
      {/* the chip names the setting only ("Snap auto"); what auto resolves to is in its tooltip and the status line */}
      <ChipSelect label="Snap" title={U.snap < 0 ? t('a.tb.snapTipAuto', { unit: snapName(autoSnap(U.bw)) }) : t('a.tb.snapTip')} value={String(U.snap)}
        options={SNAPS.map(([label, v]) => [(v === 0 ? t('a.snap.off') : label).replace(/^Snap: /, ''), String(v)])} onChange={v => A.setSnap(parseFloat(v))} />
      {/* what dragging a clip edge does (FL's stretch switch, Shift+M) */}
      <ChipSelect label={t('a.pl.edges')} title={t(U.stretchMode ? 'a.pl.modeStretchTip' : 'a.pl.modeTrimTip')} value={U.stretchMode ? '1' : '0'}
        options={[[t('a.pl.modeTrim'), '0'], [t('a.pl.modeStretch'), '1']]} onChange={v => { if ((v === '1') !== U.stretchMode) A.toggleStretchMode(); }} />
    </div>
  );
}
