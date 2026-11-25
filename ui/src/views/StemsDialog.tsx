// "Extract stems…" (Playlist audio clip menu): pick the stems, see what will happen and how long it
// takes, download the model on first use, then follow the background job here or as a chip in the
// corner while working on. The job itself lives in state/stems.ts.
import { createMemo, For, Show } from 'solid-js';
import type { StemId } from '../api/types';
import { t } from '../lib/i18n';
import { fmtBytes } from '../lib/format';
import { S } from '../state/store';
import { cancelStems, closeStems, fmtEta, fmtSpan, openStems, runStems, STEM_COLORS, STEM_IDS, STEMS, stemsEta, toggleStem } from '../state/stems';
import { DialogBoundary } from '../ui/PanelBoundary';
import { modal } from '../ui/modal';

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
  const failed = () => !!STEMS.error && STEMS.errorClip === clip()?.id && !mine();

  return (
    <>
      <Show when={clip()}>
        {c => (
          <DialogBoundary name={t('d.stems.title')} close={closeStems}>
          <div class="overlay" onPointerDown={closeStems}>
            <div class="dialog stemsdlg" aria-labelledby="stems-title" ref={el => modal(el, { close: closeStems })} onPointerDown={e => e.stopPropagation()} data-stems-dialog
              onKeyDown={e => {
                if (e.key === 'Enter' && !mine() && !(e.target instanceof HTMLButtonElement)) { e.preventDefault(); go(); }
              }}>
              <div class="stems-hd">
                <div class="stems-hd-txt">
                  <span id="stems-title" class="stems-title">{t('d.stems.title')}</span>
                  <span class="stems-sub">
                    <span class="ell" title={src()?.name}>{src()?.name}</span>
                    <span class="mono stems-len">{t('d.stems.length', { len: fmtSpan(audioS()), eta: fmtEta(stemsEta(audioS())) })}</span>
                  </span>
                </div>
                <button class="stems-x" onClick={closeStems} title={t('d.stems.close')} aria-label={t('d.stems.close')}>×</button>
              </div>
              <div class="stems-picks" role="group" aria-label={t('d.stems.title')}>
                <For each={STEM_IDS}>
                  {(s: StemId) => {
                    const on = () => (mine() ? !!job()?.stems.includes(s) : STEMS.picked.includes(s));
                    return (
                      <button type="button" class={['stem-pick', { on: on() }]} style={{ '--stem': STEM_COLORS[s] }}
                        aria-pressed={on() ? 'true' : 'false'} disabled={mine()} title={t(`d.stems.${s}.hint` as 'd.stems.drums.hint')} onClick={() => toggleStem(s)} data-stem={s}>
                        <span class="stem-dot" />
                        <span class="stem-name">{t(`d.stems.${s}` as 'd.stems.drums')}</span>
                        <span class="stem-hint">{t(`d.stems.${s}.hint` as 'd.stems.drums.hint')}</span>
                      </button>
                    );
                  }}
                </For>
              </div>
              <Show when={mine()} fallback={
                <ul class="stems-notes">
                  <li>{t('d.stems.what')}</li>
                  <Show when={stretched()}><li>{t('d.stems.stretchNote')}</li></Show>
                  <Show when={c().tune?.notes?.length}><li>{t('d.stems.tuneNote')}</li></Show>
                  <Show when={!S.path}><li>{t('d.stems.unsavedNote')}</li></Show>
                </ul>
              }>
                <div class="stems-progress" data-stems-progress>
                  <span>{stageText()}</span>
                  <div class="stems-bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(barFrac() * 100)}>
                    <div style={{ width: barFrac() * 100 + '%' }} />
                  </div>
                </div>
              </Show>
              <Show when={needModel() && !mine()}>
                <div class="stems-box">
                  <span>{t('d.stems.modelNeeded', { size: size() })}</span>
                  <span class="mono sel-text stems-lic ell" title={STEMS.model?.licenseUrl}>{t('d.stems.license')}: {STEMS.model?.licenseUrl?.replace(/^https?:\/\//, '')}</span>
                </div>
              </Show>
              <Show when={failed()}>
                <div class="stems-box err" role="alert" data-stems-error>{STEMS.error}</div>
              </Show>
              <div class="stems-foot dlgfoot">
                <Show when={mine()} fallback={
                  <>
                    <button class="smallbtn" onClick={closeStems}>{t('d.stems.close')}</button>
                    <button class="smallbtn primary" data-autofocus disabled={!!blocked() || STEMS.model == null} title={blocked() ?? ''} onClick={go} data-stems-go>
                      {failed() ? t('d.stems.retry') : label()}
                    </button>
                  </>
                }>
                  <button class="smallbtn" title={t('d.stems.backgroundTip')} onClick={closeStems}>{t('d.stems.background')}</button>
                  <button class="smallbtn" onClick={cancelStems} disabled={job()?.stage === 'place'} data-stems-cancel>{t('d.stems.cancel')}</button>
                </Show>
              </div>
            </div>
          </div>
          </DialogBoundary>
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
