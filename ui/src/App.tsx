import { AskDialog, toggleAsk } from './views/Ask';
import { t as tl } from './lib/i18n';
import { closeNotebook, NB } from './state/notebook';
import { ConfirmDialog } from './ui/Confirm';
import { createEffect, For, onCleanup, onMount, Show } from 'solid-js';
import { be, isTauri } from './api/backend';
import { baseName } from './lib/format';
import { typingIn } from './lib/keys';
import { snapRes } from './lib/snap';
import * as A from './state/actions';
import { findFx, S } from './state/store';
import { swapTake } from './state/comp';
import { closeMenu, say, setU, U } from './state/ui';
import { ContextMenu } from './ui/Menu';
import { osDragEnd, osDragOver, osDrop } from './views/Browser';
import { FxWindow } from './views/FxWindow';
import { Mixer, MixerWindow, MixView } from './views/Mixer';
import { hideMixer, leaveMixView, toggleMixer } from './state/mixView';
import { chanById, routeSelectedChannels } from './state/pattern';
import { playlistZoom } from './views/Playlist';
import { Workspace } from './views/PatDock';
import { SampleEditor } from './views/SampleEditor';
import { ChannelWindow } from './views/ChannelWindow';
import { PitchEditor } from './views/PitchEditor';
import { Export } from './views/Export';
import { Settings, VstPicker } from './views/Settings';
import { AudioToast } from './views/AudioToast';
import { Notice } from './views/Notice';
import { Updates } from './views/Updates';
import { midiStatus, startMidi } from './state/midi';
import { releaseAll, setTypingOn, typingDown, typingOn, typingUp } from './state/typingKeys';
import { chanWin, closeChannelWindow, toggleChannelWindow } from './views/ChannelWindow';
import { channels } from './state/pattern';
import { MusicDialog } from './views/MusicDialog';
import { MissingAudio } from './views/MissingAudio';
import { MidiImport } from './views/MidiImport';
import { FxPickerHost, openFxPicker } from './views/FxPicker';
import { FxChainsHost } from './views/FxChains';
import { ChannelRack } from './views/ChannelRack';
import { PianoRoll } from './views/PianoRoll';
import { renameCurrent } from './views/patternShared';
import { NotebookHost } from './views/Notebook';
import { toggleNotebook } from './state/notebook';
import { clonePattern, closeRack, closeRoll, nextEmptyPattern, patMode, pickPatternNum, rackFloating, rollFloating, stepPattern, toggleRack, toggleRoll } from './state/pattern';
import { mixFloating, topFloating, type Floating } from './state/winOrder';
import { Help, Toolbar, TOOLS } from './views/Toolbar';

/** Closes one floating window. */
function closeFloating(f: Floating) {
  switch (f.kind) {
    case 'plug': return A.closePlug(f.id!);
    case 'rack': return closeRack();
    case 'roll': return closeRoll();
    case 'mix': return hideMixer();
    case 'editor': return A.closeEditor();
    case 'chan': return closeChannelWindow();
    case 'pitch': return A.closePitchEditor();
  }
}

/** Stacking rank of a window (effect windows, floating Channel Rack, floating Piano roll) inside the window layer (1 = bottom). */
function plugRank(z: number) {
  let r = 1;
  for (const p of U.plugs) if (p.z < z) r++;
  if (rackFloating() && U.rackWin!.z < z) r++;
  if (rollFloating() && U.rollWin!.z < z) r++;
  if (mixFloating() && U.mixWin.z < z) r++;
  return r;
}

/** A click on the playlist/toolbar takes the keyboard back from a window (Channel Rack, Piano roll — floating or docked —, effects):
 * the canvas calls preventDefault on pointerdown, so focus would otherwise stay in that window and swallow the keys. */
function reclaimFocus(e: PointerEvent) {
  const ae = document.activeElement as HTMLElement | null, tg = e.target as HTMLElement;
  lastClickInWin = !!tg.closest('.win');
  if (ae && ae !== document.body && ae.closest('.win') && !lastClickInWin && !typingIn(tg)) ae.blur();
}
/** The last click was in a floating window (an effect window's title bar takes no focus, so the focus alone does not tell). */
let lastClickInWin = false;

/** The channel the typing keyboard plays: the open channel window, the selected channel while the rack or the roll is open,
 * the channel feeding the top effect window's insert, the MIDI target, else the first instrument. */
function typingTarget() {
  const cw = chanWin(); if (cw) return cw.id;
  if ((U.rackWin || U.rollWin) && U.selCh != null && channels().some(c => c.id === U.selCh)) return U.selCh;
  const top = A.topPlug(), hit = top ? findFx(top.fxId) : null;
  const cs = channels();
  const fed = hit ? cs.find(c => c.insert === hit.m.id) : undefined; if (fed) return fed.id;
  const t = midiStatus()?.target; if (t != null && cs.some(c => c.id === t)) return t;
  return cs[0]?.id ?? null;
}

/** The keyboard is in a pattern window (Channel Rack, Piano roll): its own keys win over the playlist's. */
const patWinFocused = () => !!(document.activeElement as HTMLElement | null)?.closest?.('.patwin');

function onKey(e: KeyboardEvent) {
  if (U.openingProject) { e.preventDefault(); return; }
  if (e.key === 'Alt') { e.preventDefault(); if (!U.alt) setU('alt', true); return; }
  if (typingIn(e.target as HTMLElement)) return;
  const k = e.key.toLowerCase(), m = e.ctrlKey || e.metaKey;
  if (e.code === 'Space') {
    e.preventDefault();
    // Space must not press the focused button; but a pattern window keeps the keyboard (its
    // Delete / Ctrl+C / arrows would otherwise fall through to the playlist after a play)
    const ae = document.activeElement as HTMLElement | null;
    if (ae && /^(BUTTON|SELECT|INPUT)$/.test(ae.tagName)) { const w = ae.closest<HTMLElement>('.win'); if (w) w.focus({ preventScroll: true }); else ae.blur(); }
    if (!e.repeat) void A.togglePlay();
    return;
  }
  if (m && !e.shiftKey && !e.altKey && e.code === 'KeyT') { e.preventDefault(); return setTypingOn(!typingOn()); }
  // FL's "Stop sound" (panic): stop, and cut every tail still ringing
  if (m && !e.shiftKey && !e.altKey && e.code === 'KeyH') { e.preventDefault(); return void (U.playing || U.patPlaying ? A.stop().then(() => A.silence()) : A.silence()); }
  if (typingDown(e, typingTarget)) return;
  if (e.key === 'Escape') {
    if (U.menu) return closeMenu();
    if (U.autoSel) return setU('autoSel', null);
    if (U.musicWin) return A.closeMusicAnalysis();
    if (U.missingOpen) return setU('missingOpen', false);
    if (U.midiWin) return setU('midiWin', null);
    if (U.fxPicker) return setU('fxPicker', null);
    if (U.fxChainWin) return setU('fxChainWin', null);
    if (U.vstPicker) return setU('vstPicker', null);
    if (U.settings) return setU('settings', false);
    if (U.exportOpen) return void A.cancelExportOrClose();
    if (U.help) return setU('help', false);
    const top = topFloating();
    if (NB.open && !top) return closeNotebook();
    if (U.mixView) {
      // the Mix view: Esc closes the window you are working in (last click or keyboard there), else goes back to arrange
      const inWin = lastClickInWin || !!(document.activeElement as HTMLElement | null)?.closest?.('.win');
      if (inWin && top) return closeFloating(top);
      return void leaveMixView();
    }
    // the window in front closes, whichever kind it is (FL: Esc closes the focused window)
    if (top) return closeFloating(top);
    if (U.compSel) return setU('compSel', null);
    return setU('sel', []);
  }
  // playlist zoom (FL): Page Up / Down around the pointer, Shift+1–3 presets, Shift+4 the whole song.
  // Not while the Channel Rack or the Piano roll has the keyboard: they zoom their own view.
  if (!m && !e.altKey && !patWinFocused() && (e.key === 'PageUp' || e.key === 'PageDown')) { e.preventDefault(); return playlistZoom.step(e.key === 'PageUp' ? 1.25 : 0.8); }
  if (e.shiftKey && !m && !e.altKey && !patWinFocused() && /^Digit[1-4]$/.test(e.code)) {
    e.preventDefault();
    const n = +e.code.slice(5);
    return n === 4 ? playlistZoom.fit() : playlistZoom.preset(n - 1);
  }
  if (e.key === 'F1') { e.preventDefault(); return toggleAsk(); }
  if (e.key === 'F8') { e.preventDefault(); return e.altKey ? setU('pickOpen', !U.pickOpen) : toggleNotebook(); }
  // FL Studio's window keys: F6 Channel rack, F7 Piano roll; the effect picker sits next to the mixer (Shift+F9)
  if (e.key === 'F9' && e.shiftKey) { e.preventDefault(); return U.fxPicker ? setU('fxPicker', null) : openFxPicker(U.mixSel); }
  if (e.key === 'F7') { e.preventDefault(); return toggleRoll(); }
  if (e.key === 'F6') { e.preventDefault(); return toggleRack(); }
  // the pattern selector: F4 next empty pattern, F2 rename, numpad 1–9 / + / − pick
  if (e.key === 'F4') { e.preventDefault(); return nextEmptyPattern(); }
  if (e.key === 'F2') { e.preventDefault(); const r = document.querySelector('[data-patsel]')?.getBoundingClientRect(); return renameCurrent(r?.left ?? window.innerWidth / 2, r ? r.bottom + 4 : 80); }
  if (!m && !e.altKey && e.code === 'NumpadAdd') { e.preventDefault(); return stepPattern(1); }
  if (!m && !e.altKey && e.code === 'NumpadSubtract') { e.preventDefault(); return stepPattern(-1); }
  if (!m && !e.altKey && /^Numpad[1-9]$/.test(e.code)) { e.preventDefault(); return pickPatternNum(+e.code.slice(6)); }
  if (m && e.shiftKey && k === 'c') { e.preventDefault(); return clonePattern(); }
  if (e.key === 'F5') { e.preventDefault(); return U.plugsHidden ? A.showPlugs() : A.playlistFront(); }
  if (e.key === 'F9') { e.preventDefault(); return toggleMixer(); }
  // FL: route the selected Channel Rack channels to the selected mixer insert (Shift: one per insert from it on)
  if (m && !e.altKey && e.code === 'KeyL') { e.preventDefault(); return routeSelectedChannels(U.mixSel, e.shiftKey); }
  // FL's export in place (consolidate): the selected clips rendered onto a new track below
  if (m && e.altKey && !e.shiftKey && e.code === 'KeyE') { e.preventDefault(); return void A.exportInPlace(); }
  if (m && k === 'z') { e.preventDefault(); return void (e.shiftKey ? A.redo() : A.undo()); }
  if (m && k === 'y') { e.preventDefault(); return void A.redo(); }
  if (m && k === 'n') { e.preventDefault(); return void A.projectNew(); }
  if (m && k === 'o') { e.preventDefault(); return void A.projectOpen(); }
  if (m && k === 's') { e.preventDefault(); return void A.projectSave(e.shiftKey); }
  if (m && e.shiftKey && k === 'r') { e.preventDefault(); return setU('exportOpen', true); }
  // the Mix view hides the Playlist: its clip keys must not edit an arrangement nobody sees
  if (U.mixView && ((m && !e.altKey && 'abdcxvm'.includes(k)) || e.key === 'Delete' || e.key === 'Backspace' || (e.shiftKey && e.key.startsWith('Arrow')))) return;
  if (m && k === 'a') { e.preventDefault(); return A.selectAll(); }
  if (m && (k === 'b' || k === 'd')) { e.preventDefault(); return A.duplicate(); } // Ctrl+B (FL) and Ctrl+D (Ableton/Reaper); preventDefault also stops the browser bookmark
  if (m && k === 'c') { e.preventDefault(); return A.copy(); }
  if (m && k === 'x') { e.preventDefault(); return A.cut(); }
  if (m && k === 'v') { e.preventDefault(); return A.paste(); }
  if (m && k === 'm') { e.preventDefault(); return A.toggleMuteSel(); }
  if (m && (k === '=' || k === '+')) { e.preventDefault(); return A.zoomAt(1.25); }
  if (m && k === '-') { e.preventDefault(); return A.zoomAt(0.8); }
  if (e.key === 'Delete' || e.key === 'Backspace') { e.preventDefault(); return U.autoSel?.idx.length ? A.deleteAutoPoints() : A.delIds([...U.sel]); }
  if (e.key === 'Home') { e.preventDefault(); return A.seek(0); }
  if (e.key === '?') return setU('help', !U.help);
  if (m && !e.shiftKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown') && swapTake(e.key === 'ArrowUp' ? -1 : 1)) { e.preventDefault(); return; }
  if (e.shiftKey && e.key.startsWith('Arrow')) {
    e.preventDefault();
    const u = snapRes(U.snap, U.bw) || 0.25;
    if (e.key === 'ArrowLeft') A.nudge(-u, 0);
    if (e.key === 'ArrowRight') A.nudge(u, 0);
    if (e.key === 'ArrowUp') A.nudge(0, -1);
    if (e.key === 'ArrowDown') A.nudge(0, 1);
    return;
  }
  // FL's Shift+M: dragging a clip edge stretches instead of trimming
  if (e.shiftKey && !m && !e.altKey && e.code === 'KeyM' && !patWinFocused()) { e.preventDefault(); return A.toggleStretchMode(); }
  if (!m && !e.altKey) {
    if (k === 'r') return A.toggleRec();
    if (k === 'l') return A.switchMode(!patMode());
    if (k === 'f') { setU('follow', !U.follow); return say(U.follow ? tl('a.app.followOn') : tl('a.app.followOff')); }
    const t = TOOLS.find(x => x[1].toLowerCase() === k);
    if (t) { setU('tool', t[0]); say(tl('a.app.tool', { name: tl(t[2]) })); }
  }
}
const onKeyUp = (e: KeyboardEvent) => {
  typingUp(e);
  if (e.key === 'Alt') { e.preventDefault(); setU('alt', false); }
  if (e.code === 'Space' && (e.target as HTMLElement).tagName === 'BUTTON') e.preventDefault(); // no button click on Space
};
const onBlur = () => { if (U.alt) setU('alt', false); releaseAll(); };

export function App() {
  onMount(() => {
    startMidi();
    // the gear in a plugin window's header: WAVE's own window for that plugin channel or effect
    const offSettings = be().on('plugin-settings', ({ id }) => {
      if (chanById(id)) toggleChannelWindow(id, true);
      else if (findFx(id)) A.openPlug(id, true);
    });
    onCleanup(() => void offSettings.then(off => off()));
    if (isTauri()) {
      void import('@tauri-apps/api/window').then(async ({ getCurrentWindow }) => {
        const w = getCurrentWindow();
        let closing = false;
        await w.onCloseRequested(async ev => {
          if (closing) return;
          ev.preventDefault();
          if (await A.closeWindowGuarded()) { closing = true; await w.destroy(); }
        });
      }).catch(() => {});
    }
    window.addEventListener('keydown', onKey);
    window.addEventListener('pointerdown', reclaimFocus, true);
    window.addEventListener('keyup', onKeyUp);
    window.addEventListener('blur', onBlur);
    let offDrop: (() => void) | null = null;
    be().onOsDrop(ev => {
      if (U.openingProject) return;
      // over the Channel Rack the row it lands on is marked; anywhere else the Playlist's drop hint shows
      if (ev.type === 'over') { const rack = osDragOver(ev.x, ev.y); if (U.dragOver === rack) setU('dragOver', !rack); }
      else if (ev.type === 'leave') { osDragEnd(); setU('dragOver', false); }
      else { setU('dragOver', false); if (ev.paths.length) osDrop(ev.paths, ev.x, ev.y, U.alt); else osDragEnd(); }
    }).then(u => { offDrop = u; });
    onCleanup(() => {
      window.removeEventListener('keydown', onKey); window.removeEventListener('pointerdown', reclaimFocus, true); window.removeEventListener('keyup', onKeyUp); window.removeEventListener('blur', onBlur);
      offDrop?.();
    });
  });

  // effects whose windows are open → analysis in the engine (≤ 8)
  let lastWatch = '';
  createEffect(() => {
    // the 8 front-most windows (engine limit), in a stable order so focusing does not re-send
    const ids = (U.plugsHidden ? [] : [...U.plugs]).sort((a, b) => b.z - a.z).map(p => p.fxId).filter(id => { const f = findFx(id)?.f; return f && f.type !== 'plugin'; }).slice(0, 8).sort((a, b) => a - b);
    const key = ids.join(',');
    if (key !== lastWatch) { lastWatch = key; be().watchFx(ids).catch(e => console.warn(e)); }
  });
  // close windows of effects that disappeared (undo, CLI, …)
  createEffect(() => {
    const gone = U.plugs.filter(p => !findFx(p.fxId));
    if (gone.length) setU('plugs', U.plugs.filter(p => findFx(p.fxId)));
    const ed = U.editor;
    if (ed && !(ed.kind === 'clip'
      ? S.project.clips.some(c => c.id === ed.id && c.kind === 'audio')
      : S.project.channels?.some(c => c.id === ed.id && c.kind === 'sample'))) A.closeEditor();
    if (U.pitchEd && !S.project.clips.some(c => c.id === U.pitchEd!.clipId)) setU('pitchEd', null);
    if (U.musicWin?.clipId != null && !S.project.clips.some(c => c.id === U.musicWin!.clipId)) setU('musicWin', null);
    if (U.musicWin?.fxId != null && !findFx(U.musicWin!.fxId)) setU('musicWin', null);
  });
  createEffect(() => { be().setTitle((S.path ? baseName(S.path) : tl('a.tb.untitled')) + (S.dirty ? ' •' : '') + ' — WAVE'); });

  return (
    <>
    <div class="app" inert={U.openingProject} onContextMenu={e => e.preventDefault()}>
      <Toolbar />
      <div class="stage">
        <Workspace />
        <Mixer />
        <Show when={U.mixView}><MixView /></Show>
      </div>
      {/* the editors float outside the window layer (own z order, see outerZ); F5 tucks them away too */}
      <div class="outerwins" classList={{ hidden: U.plugsHidden }}>
        <SampleEditor />
        <ChannelWindow />
        <PitchEditor />
      </div>
      <div class="winlayer" classList={{ hidden: U.plugsHidden, mv: U.mixView }}>
        <For each={U.plugs}>{pw => <FxWindow pw={pw} z={plugRank(pw.z)} />}</For>
        <Show when={!U.rackDock}><ChannelRack z={U.rackWin ? plugRank(U.rackWin.z) : 1} /></Show>
        <Show when={!U.rollDock}><PianoRoll z={U.rollWin ? plugRank(U.rollWin.z) : 1} /></Show>
        <Show when={mixFloating()}><MixerWindow z={plugRank(U.mixWin.z)} /></Show>
      </div>
      <NotebookHost />
      <Show when={U.ghost}>{g => <div class="ghost" style={{ left: g().x + 'px', top: g().y + 'px', 'box-shadow': `inset 0 0 0 1px ${g().color},0 6px 16px rgba(0,0,0,.45)` }}>{g().name}</div>}</Show>
      <ContextMenu />
      <Help />
      <ConfirmDialog />
      <Export />
      <Settings />
      <Updates />
      <AskDialog />
      <VstPicker />
      <FxPickerHost />
      <FxChainsHost />
      <MusicDialog />
      <MidiImport />
      <AudioToast />
      <Notice />
      <MissingAudio />
    </div>
    <Show when={U.openingProject}>
      <div class="overlay" style={{ 'z-index': 400 }}>
        <div class="dialog" role="status">{tl('d.act.opening')}</div>
      </div>
    </Show>
    </>
  );
}
