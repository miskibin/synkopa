// "Extract stems…" (Playlist audio clip menu): pick the stems, see what will happen and how long it
// takes, download the model on first use, then follow the background job here or as a chip in the
// corner while working on. The job itself lives in state/stems.ts.
import { createMemo, For, Show } from 'solid-js';
import type { StemId } from '../api/types';
import { t } from '../lib/i18n';
import { fmtBytes } from '../lib/format';
import { S } from '../state/store';
import { cancelStems, closeStems, fmtEta, fmtSpan, openStems, runStems, STEM_COLORS, STEM_IDS, STEMS, stemsEta, toggleStem } from '../state/stems';

const pct = (f: number) => Math.round(Math.max(0, Math.min(1, f)) * 100) + ' %';
const STAGE_KEY = { download: 'd.stems.stage.download', prepare: 'd.stems.stage.prepare', load: 'd.stems.stage.load', separate: 'd.stems.stage.separate', place: 'd.stems.stage.place' } as const;

export function StemsDialog() {
  const clip = createMemo(() => (STEMS.win != null ? S.project.clips.find(c => c.id === STEMS.win) : undefined));
  const src = () => S.project.sources.find(s => s.id === clip()?.ref);
  /** Seconds of the source the clip plays (what is separated, plus a little context). */
  const audioS = () => {
    const c = clip(), s = src(); if (!c || !s) return 0;
    const spb = 60 / S.project.bpm, st = c.stretch ?? 1;
    return Math.max(0, Math.min(s.dur, (c.offset + c.len) * spb / st) - c.offset * spb / st);
  };
  const job = () => STEMS.job;
  const mine = () => job()?.clipId === STEMS.win;
  const n = () => STEMS.picked.length;
  const blocked = (): string | null => {
    const c = clip(); if (!c) return null;
    if (job() && !mine()) return t('d.stems.busyOther');
    if (c.ref in S.missing) return t('d.stems.missing');
    if (audioS() > 1800) return t('d.stems.tooLong');
    if (!n()) return t('d.stems.none');
    return null;
  };
  const size = () => fmtBytes(STEMS.model?.bytes ?? 0);
  const needModel = () => STEMS.model != null && !STEMS.model.installed;
  const label = () => (needModel() ? t('d.stems.downloadExtract', { size: size() }) : n() === 1 ? t('d.stems.extract1') : t('d.stems.extract', { n: n() }));
  const stretched = () => { const c = clip(); return !!c && ((c.stretch ?? 1) !== 1 || !!c.pitch || !!c.gain || !!c.fadeIn || !!c.fadeOut); };
  const go = () => { const c = clip(); if (c && !blocked() && !job()) void runStems(c.id); };
  const stageText = () => { const j = job(); return j ? t(STAGE_KEY[j.stage], { pct: pct(j.frac) }) : ''; };
  // the download and the separation fill one bar each; the other stages are short, shown as an indeterminate sliver
  const barFrac = () => { const j = job(); return !j ? 0 : j.stage === 'download' || j.stage === 'separate' ? j.frac : j.stage === 'place' ? 1 : 0.02; };

  return (
    <>
      <Show when={clip()}>
        {c => (
          <div class="overlay" onPointerDown={closeStems}>
            <div class="dialog" style={{ width: 'min(480px, 92vw)' }} onPointerDown={e => e.stopPropagation()} data-stems-dialog
              onKeyDown={e => { if (e.key === 'Enter' && !mine()) { e.preventDefault(); go(); } }}>
              <div class="hd">
                <span class="ell" title={src()?.name}>{t('d.stems.title', { name: src()?.name ?? '' })}</span>
                <button onClick={closeStems} title={t('d.stems.close')}>×</button>
              </div>
              <div style={{ display: 'flex', 'flex-direction': 'column', gap: '12px' }}>
                <div style={{ display: 'grid', 'grid-template-columns': '1fr 1fr', gap: '6px' }} role="group">
                  <For each={STEM_IDS}>
                    {(s: StemId) => {
                      const on = () => STEMS.picked.includes(s);
                      return (
                        <label class="stem-pick" classList={{ on: on() }} title={t(`d.stems.${s}.hint` as 'd.stems.drums.hint')}
                          style={{ display: 'flex', 'align-items': 'center', gap: '8px', padding: '7px 9px', border: '1px solid var(--line)', 'border-radius': '6px', background: on() ? 'var(--bg1)' : 'transparent', cursor: mine() ? 'default' : 'pointer', opacity: mine() && !job()?.stems.includes(s) ? 0.45 : 1 }}>
                          <input type="checkbox" checked={mine() ? job()!.stems.includes(s) : on()} disabled={mine()} onChange={() => toggleStem(s)} data-stem={s} />
                          <span style={{ width: '8px', height: '18px', 'border-radius': '2px', background: STEM_COLORS[s], 'flex-shrink': 0 }} />
                          <span style={{ display: 'flex', 'flex-direction': 'column', 'min-width': 0 }}>
                            <span style={{ 'font-weight': 600 }}>{t(`d.stems.${s}` as 'd.stems.drums')}</span>
                            <span class="ell" style={{ 'font-size': '10px', color: 'var(--fg4)' }}>{t(`d.stems.${s}.hint` as 'd.stems.drums.hint')}</span>
                          </span>
                        </label>
                      );
                    }}
                  </For>
                </div>
                <div class="note" style={{ display: 'flex', 'flex-direction': 'column', gap: '4px' }}>
                  <span class="mono" style={{ color: 'var(--fg3)' }}>{t('d.stems.length', { len: fmtSpan(audioS()), eta: fmtEta(stemsEta(audioS())) })}</span>
                  <span>{t('d.stems.what')}</span>
                  <Show when={stretched()}><span>{t('d.stems.stretchNote')}</span></Show>
                  <Show when={c().tune?.notes?.length}><span>{t('d.stems.tuneNote')}</span></Show>
                  <Show when={!S.path}><span>{t('d.stems.unsavedNote')}</span></Show>
                </div>
                <Show when={needModel() && !mine()}>
                  <div class="note" style={{ padding: '8px 10px', background: 'var(--bg1)', border: '1px solid var(--line)', 'border-radius': '6px' }}>
                    <div>{t('d.stems.modelNeeded', { size: size() })}</div>
                    <div class="mono sel-text" style={{ 'font-size': '10px', color: 'var(--fg4)', 'margin-top': '3px' }}>{t('d.stems.license')}: {STEMS.model?.licenseUrl}</div>
                  </div>
                </Show>
                <Show when={STEMS.error && STEMS.errorClip === c().id && !mine()}>
                  <div class="note err" role="alert" data-stems-error>{STEMS.error}</div>
                </Show>
                <Show when={mine()}>
                  <div style={{ display: 'flex', 'flex-direction': 'column', gap: '6px' }} data-stems-progress>
                    <div class="row" style={{ 'justify-content': 'space-between' }}>
                      <span>{stageText()}</span>
                    </div>
                    <div style={{ height: '6px', background: 'var(--bg1)', border: '1px solid var(--line)', 'border-radius': '3px', overflow: 'hidden' }}
                      role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(barFrac() * 100)}>
                      <div style={{ height: '100%', width: barFrac() * 100 + '%', background: 'var(--blue)', transition: 'width 0.3s' }} />
                    </div>
                  </div>
                </Show>
                <div class="row" style={{ gap: '8px', 'justify-content': 'flex-end' }}>
                  <Show when={mine()} fallback={
                    <>
                      <button class="smallbtn" style={{ height: '28px', padding: '0 12px' }} onClick={closeStems}>{t('d.stems.close')}</button>
                      <button class="smallbtn primary" style={{ height: '28px', padding: '0 12px' }} disabled={!!blocked() || STEMS.model == null} title={blocked() ?? ''} onClick={go} data-stems-go>
                        {STEMS.error && STEMS.errorClip === c().id ? t('d.stems.retry') : label()}
                      </button>
                    </>
                  }>
                    <button class="smallbtn" style={{ height: '28px', padding: '0 12px' }} title={t('d.stems.backgroundTip')} onClick={closeStems}>{t('d.stems.background')}</button>
                    <button class="smallbtn" style={{ height: '28px', padding: '0 12px' }} onClick={cancelStems} disabled={job()?.stage === 'place'} data-stems-cancel>{t('d.stems.cancel')}</button>
                  </Show>
                </div>
              </div>
            </div>
          </div>
        )}
      </Show>
      {/* a job whose dialog is hidden: its progress in the corner, a click brings the dialog back */}
      <Show when={job() && STEMS.win == null ? job() : null}>
        {j => (
          <div class="stems-chip" role="status" title={t('d.stems.chipTip')} data-stems-chip onClick={() => openStems(j().clipId)}
            style={{ position: 'fixed', right: '14px', bottom: '14px', 'z-index': 900, display: 'flex', 'align-items': 'center', gap: '10px', padding: '8px 10px', background: 'var(--bg2)', border: '1px solid var(--line2)', 'border-radius': '8px', 'box-shadow': '0 6px 20px rgba(0,0,0,.35)', cursor: 'pointer', 'max-width': 'min(360px, calc(100vw - 28px))' }}>
            <div style={{ display: 'flex', 'flex-direction': 'column', gap: '5px', 'min-width': 0, flex: 1 }}>
              <span class="ell" style={{ 'font-weight': 600 }}>{t('d.stems.chip', { name: j().name })}</span>
              <span class="ell" style={{ 'font-size': '11px', color: 'var(--fg3)' }}>{stageText()}</span>
              <div style={{ height: '4px', width: '200px', 'max-width': '100%', background: 'var(--bg1)', 'border-radius': '2px', overflow: 'hidden' }}>
                <div style={{ height: '100%', width: barFrac() * 100 + '%', background: 'var(--blue)', transition: 'width 0.3s' }} />
              </div>
            </div>
            <button class="smallbtn" style={{ height: '24px', padding: '0 8px' }} disabled={j().stage === 'place'} onClick={e => { e.stopPropagation(); cancelStems(); }}>{t('d.stems.cancel')}</button>
          </div>
        )}
      </Show>
    </>
  );
}
