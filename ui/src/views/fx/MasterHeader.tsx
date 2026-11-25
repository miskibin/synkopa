// The Master's listen row: the button that makes it listen to the whole song (with progress and
// Stop while it runs) and what the last listen found, in one line: how much was heard, loudness
// in → out, the true peak and the loudness range in → out. Before the first listen it says the
// controls are neutral, so nobody mistakes the untouched effect for a finished master.
import { Show } from 'solid-js';
import type { Fx } from '../../api/types';
import { t } from '../../lib/i18n';
import { MASTER_TARGETS, MAX_CLEAN_CUT_DB, mmss } from '../../lib/master';
import * as A from '../../state/actions';

export function MasterHeader(props: { fx: Fx }) {
  const prog = () => A.masterListening(props.fx.id);
  const L = () => props.fx.listen;
  const stale = () => { const l = L(); return !!l && l.target !== Math.round(props.fx.params.target ?? 2); };
  return (
    <div class="ctlrow msrow" data-master-listen>
      <Show when={prog() == null} fallback={
        <>
          <div class="msprog" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round((prog() ?? 0) * 100)}>
            <div class="msbar" style={{ width: `${Math.round((prog() ?? 0) * 100)}%` }} />
            <span>{t('c.ms.listening', { p: Math.round((prog() ?? 0) * 100) })}</span>
          </div>
          <button class="smallbtn" onClick={() => A.masterListenCancel()}>{t('c.ms.cancel')}</button>
        </>
      }>
        <button class={['smallbtn msgo', { on: !L() || stale() }]} title={t('c.ms.listenTip')} onClick={() => void A.masterListen(props.fx.id)}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M3 14v-2a9 9 0 0 1 18 0v2" /><rect x="2.5" y="14" width="5" height="7" rx="1.5" /><rect x="16.5" y="14" width="5" height="7" rx="1.5" />
          </svg>
          {L() ? t('c.ms.relisten') : t('c.ms.listen')}
        </button>
        <span class="mssum ell">
          <Show when={L()} fallback={t('c.ms.never')}>
            {l => (
              <>
                {t('c.ms.summary', {
                  dur: mmss(l().seconds), inL: l().inLufs.toFixed(1), outL: l().outLufs.toFixed(1),
                  outTp: l().outTp.toFixed(1), inLra: l().inLra.toFixed(1), outLra: l().outLra.toFixed(1),
                })}
                <Show when={l().short}><b class="mswarn"> · {t('c.ms.shortTag')}</b></Show>
                <Show when={l().guarded}><b class="mswarn" title={t('c.ms.guardTip', { max: MAX_CLEAN_CUT_DB })}> · {t('c.ms.guard', { target: MASTER_TARGETS[l().target] ?? -12, db: (l().peakCut ?? 0).toFixed(1) })}</b></Show>
                <Show when={stale()}><b class="mswarn"> · {t('c.ms.stale', { target: MASTER_TARGETS[Math.round(props.fx.params.target ?? 2)] ?? -12 })}</b></Show>
              </>
            )}
          </Show>
        </span>
      </Show>
    </div>
  );
}
