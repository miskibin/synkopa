// "Import MIDI": the parts of a .mid file (track × channel) with what each becomes in WAVE,
// chosen with checkboxes; optionally the file's tempo. Imports as one new pattern on the
// Playlist, or — opened from a channel — one part's notes into that channel. One undo step.
import { createEffect, createSignal, For, on, Show } from 'solid-js';
import { be } from '../api/backend';
import type { MidiInfo, MidiPart } from '../api/types';
import { t } from '../lib/i18n';
import { barsText, notesText } from '../lib/pattern';
import { tName } from '../lib/i18n';
import { enqueue, resync } from '../state/edit';
import { openRoll, setCurPattern } from '../state/pattern';
import { S } from '../state/store';
import { say, setU, U } from '../state/ui';

export function MidiImport() {
  const [info, setInfo] = createSignal<MidiInfo | null>(null);
  const [err, setErr] = createSignal<string | null>(null);
  const [picked, setPicked] = createSignal<Set<number>>(new Set());
  const [tempo, setTempo] = createSignal(false);
  const [replace, setReplace] = createSignal(true);
  const [busy, setBusy] = createSignal(false);
  let req = 0, importBtn: HTMLButtonElement | undefined;

  const into = () => U.midiWin?.into ?? null;
  const close = () => setU('midiWin', null);
  createEffect(on(() => U.midiWin?.path, path => {
    setInfo(null); setErr(null); setPicked(new Set<number>());
    if (!path) return;
    const id = ++req;
    be().midiInspect(path).then(f => {
      if (id !== req) return;
      setInfo(f);
      const withNotes = f.parts.filter(p => p.notes > 0);
      // into a channel: the first melodic part (drum notes rarely belong in a pitched channel)
      setPicked(new Set(into() ? [(withNotes.find(p => !p.drums) ?? withNotes[0])?.index].filter((x): x is number => x != null) : withNotes.map(p => p.index)));
      // a fresh project takes the file's tempo; one with an arrangement keeps its own
      setTempo(f.bpm != null && Math.abs(f.bpm - f.projectBpm) > 0.005 && !S.project.clips.length);
      queueMicrotask(() => importBtn?.focus());
    }).catch(e => { if (id === req) setErr(String(e)); });
  }));

  const toggle = (p: MidiPart) => setPicked(s => {
    if (into()) return new Set([p.index]);
    const n = new Set(s); if (n.has(p.index)) n.delete(p.index); else n.add(p.index); return n;
  });
  const becomes = (p: MidiPart) => {
    const b = p.becomes;
    if (into()) return t('d.midi.intoCh', { name: S.project.channels?.find(c => c.id === into()!.ch)?.name ?? '?' });
    if (b.kind === 'drums') return t('d.midi.drums', { list: b.drums.map(d => d.name).join(', ') });
    if (b.kind === 'keys') return 'WAVE Keys';
    return tName(b.preset);
  };
  const fileName = () => (U.midiWin?.path.split(/[\\/]/).pop() ?? '');

  const run = async () => {
    const w = U.midiWin, f = info();
    if (!w || !f || busy() || !picked().size) return;
    setBusy(true);
    try {
      await enqueue(() => Promise.resolve());
      const parts = [...picked()].sort((a, b) => a - b);
      const r = await be().midiImport(w.into
        ? { path: w.path, parts, into: { ...w.into, replace: replace() } }
        : { path: w.path, parts, setTempo: tempo(), beat: w.beat, track: w.track });
      const name = fileName();
      close();
      // the engine's state with the new pattern and channels, before switching to them
      await resync();
      if (w.into) { say(t('d.midi.saidInto', { n: r.notes ?? 0 })); return; }
      setCurPattern(r.patternId, true);
      say(t('d.midi.said', { name, n: r.channelIds.length, bars: barsText((r.len ?? 4) / 4) }) + (r.tempoSet ? ' · ' + t('d.midi.saidTempo', { bpm: f.bpm!.toFixed(2).replace(/\.?0+$/, '') }) : ''));
      openRoll(r.channelIds.find(id => S.project.channels?.find(c => c.id === id)?.kind !== 'sample') ?? r.channelIds[0]);
    } catch (e) { setErr(String(e)); }
    finally { setBusy(false); }
  };

  return (
    <Show when={U.midiWin}>
      <div class="overlay" onPointerDown={close}>
        <div class="dialog mididlg" role="dialog" aria-modal="true" aria-label={t('d.midi.title', { name: fileName() })} onPointerDown={e => e.stopPropagation()}
          onKeyDown={e => { if (e.key === 'Enter' && !e.repeat && (e.target as HTMLElement).tagName !== 'BUTTON') { e.preventDefault(); void run(); } }} data-midi-dialog>
          <div class="hd">
            <span title={U.midiWin?.path}>{into() ? t('d.midi.titleInto', { name: fileName() }) : t('d.midi.title', { name: fileName() })}</span>
            <button onClick={close} aria-label={t('d.midi.cancel')}>×</button>
          </div>
          <Show when={!info() && !err()}><div class="row" style={{ gap: '8px', padding: '18px 0', color: 'var(--fg3)' }}><span class="spin" />{t('d.midi.reading')}</div></Show>
          <Show when={info()}>
            {f => (
              <>
                <div class="note">{[f().bpm != null ? f().bpm!.toFixed(2).replace(/\.?0+$/, '') + ' BPM' : t('d.midi.noTempo'), barsText(Math.ceil(f().end / 4)), t('d.midi.parts', { n: f().parts.length })].join(' · ')}</div>
                <For each={f().warnings}>{w => <div class="note warn">{w}</div>}</For>
                <ul class="midilist" role={into() ? 'radiogroup' : 'group'}>
                  <For each={f().parts}>
                    {p => (
                      <li classList={{ off: !picked().has(p.index), empty: !p.notes }}>
                        <label class="row">
                          <input type={into() ? 'radio' : 'checkbox'} name="midipart" checked={picked().has(p.index)} disabled={!p.notes || busy()} onChange={() => toggle(p)} />
                          <span class="midiname" title={p.name}>{p.name}</span>
                          <span class="tag9" title={t('d.midi.chTip')}>{'CH ' + p.channel}</span>
                          <span class="note midigm" title={p.gmName}>{p.gmName}</span>
                          <span class="note mono">{notesText(p.notes)}</span>
                        </label>
                        <div class="midito note" title={becomes(p)}>→ {becomes(p)}</div>
                      </li>
                    )}
                  </For>
                </ul>
                <Show when={into()} fallback={
                  <Show when={f().bpm != null && Math.abs(f().bpm! - f().projectBpm) > 0.005}>
                    <label class="row midiopt"><input type="checkbox" checked={tempo()} disabled={busy()} onChange={e => setTempo(e.currentTarget.checked)} />
                      {t('d.midi.tempo', { bpm: f().bpm!.toFixed(2).replace(/\.?0+$/, ''), now: f().projectBpm.toFixed(2).replace(/\.?0+$/, '') })}</label>
                  </Show>
                }>
                  <label class="row midiopt"><input type="checkbox" checked={replace()} disabled={busy()} onChange={e => setReplace(e.currentTarget.checked)} />{t('d.midi.replace')}</label>
                </Show>
              </>
            )}
          </Show>
          <Show when={err()}><div class="note err" style={{ 'margin-top': '8px' }}>{err()}</div></Show>
          <div class="row" style={{ 'justify-content': 'flex-end', gap: '8px', 'margin-top': '14px' }}>
            <button class="smallbtn" onClick={close}>{t('d.midi.cancel')}</button>
            <button ref={importBtn} class="smallbtn primary" disabled={!info() || !picked().size || busy()} onClick={() => void run()}>
              {busy() ? t('d.midi.importing') : t('d.midi.import', { n: picked().size })}
            </button>
          </div>
        </div>
      </div>
    </Show>
  );
}
