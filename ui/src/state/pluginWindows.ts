// A hosted plugin's own window (FL Studio's wrapper): opening an external instrument or effect
// shows the plugin's interface at once, in a native window under WAVE's header (name, a gear,
// close). The gear opens WAVE's window for it (parameters, level, routing). A plugin with no
// interface of its own, or a session that cannot show one (no X11 display), opens WAVE's window
// instead, as before.
import { createSignal } from 'solid-js';
import { be } from '../api/backend';
import { PLUGIN_FORMAT_LABEL, type Id, type PluginRef } from '../api/types';
import { t } from '../lib/i18n';
import { ERR_NO_EDITOR } from '../api/errors';
import { chanById } from './pattern';
import { findFx, insName, S } from './store';
import { say } from './ui';

/** The error of a plugin without a window of its own (plugins.rs `NO_EDITOR`). */
export const isNoEditor = (e: unknown) => String(e).replace(/^Error: /, '').startsWith(ERR_NO_EDITOR);

/** Plugin classes known to have no window of their own (this session). */
const noEditorCids = new Set<string>();
export const knownWithoutEditor = (ref?: PluginRef) => !!ref && noEditorCids.has(ref.cid);

const [open, setOpen] = createSignal<ReadonlySet<Id>>(new Set());
/** Whether WAVE shows the plugin's own window for this effect or channel now. */
export const editorOpen = (id: Id) => open().has(id);

let editors: Promise<boolean> | null = null;
/** Whether plugin windows can be shown in this session (Windows; Linux with X11 / XWayland). */
export function editorsHere(): Promise<boolean> {
  editors ??= be().appInfo().then(i => i.pluginEditors).catch(() => false);
  return editors;
}

/** The hosted plugin behind an effect or a plugin channel. */
export function pluginOf(id: Id): { ref: PluginRef; instrument: boolean } | null {
  const ff = findFx(id);
  if (ff?.f.type === 'plugin' && ff.f.plugin) return { ref: ff.f.plugin, instrument: false };
  const c = chanById(id);
  return c?.kind === 'plugin' ? { ref: c.plugin, instrument: true } : null;
}

/** What WAVE's header above the plugin says: the channel or effect, and the format and kind. */
export function pluginLabel(id: Id): { title: string; subtitle: string } {
  const p = pluginOf(id);
  if (!p) return { title: '', subtitle: '' };
  const fmt = PLUGIN_FORMAT_LABEL[p.ref.format] ?? p.ref.format;
  if (p.instrument) return { title: chanById(id)?.name || p.ref.name, subtitle: t('c.fx.hdrInstrument', { fmt }) };
  const ff = findFx(id);
  return { title: p.ref.name, subtitle: ff ? t('c.fx.hdrEffect', { fmt, insert: insName(ff.m) }) : fmt };
}

const mark = (id: Id, on: boolean) => setOpen(s => { const n = new Set(s); if (on) n.add(id); else n.delete(id); return n; });

// whether a plugin's window is pinned on top of the app survives a restart, per plugin class
// (the header's pin toggles at runtime; the app reports it back through `plugin-pin`)
const PIN_KEY = 'wave.pluginPin';
const pinPrefs = (): Record<string, boolean> => { try { return JSON.parse(localStorage.getItem(PIN_KEY) ?? '{}'); } catch { return {}; } };
const pinPref = (cid: string) => !!pinPrefs()[cid];
function setPinPref(cid: string, pinned: boolean) {
  const m = pinPrefs();
  if (pinned) m[cid] = true; else delete m[cid];
  try { localStorage.setItem(PIN_KEY, JSON.stringify(m)); } catch { /* private window */ }
}
/** Called by App for the header pin's report (the window has already applied it). */
export function reportPin(id: Id, pinned: boolean) {
  const p = pluginOf(id);
  if (p) setPinPref(p.ref.cid, pinned);
}

// the user may close a plugin window with its own X (or the plugin may go): follow it
let poll = 0;
function watch() {
  if (poll || !open().size) return;
  poll = window.setInterval(() => {
    const ids = [...open()];
    if (!ids.length) { clearInterval(poll); poll = 0; return; }
    for (const id of ids) {
      if (!pluginOf(id)) { mark(id, false); continue; }
      void be().pluginEditorOpen(id).then(o => { if (!o) mark(id, false); }).catch(() => mark(id, false));
    }
  }, 700);
}

/** Open (or bring forward) the plugin's own window. Rejects when it has none or cannot show one. */
export async function openEditor(id: Id): Promise<void> {
  const p = pluginOf(id);
  if (!p) throw new Error(`${id} is not a plugin`);
  try {
    await be().pluginEditor(id, true, pluginLabel(id), pinPref(p.ref.cid));
  } catch (e) {
    if (isNoEditor(e)) noEditorCids.add(p.ref.cid);
    throw e;
  }
  mark(id, true);
  watch();
}

export async function closeEditor(id: Id): Promise<void> {
  mark(id, false);
  await be().pluginEditor(id, false).catch(() => {});
}

/** The engine makes a new plugin's instance a moment after the edit: wait for it (≤ ~2 s). */
const notLoadedYet = (e: unknown) => /not a loaded plugin/.test(String(e));
const pause = (ms: number) => new Promise(r => setTimeout(r, ms));

/**
 * Open an external plugin the FL way: its own window, else (no window of its own, none can be
 * shown here, or it failed to load) WAVE's window through `fallback`.
 */
export async function showPlugin(id: Id, fallback: () => void): Promise<void> {
  const p = pluginOf(id);
  if (!p || knownWithoutEditor(p.ref) || S.fxErrors[id] || !(await editorsHere())) return fallback();
  for (let tries = 0; ; tries++) {
    try {
      await openEditor(id);
      return;
    } catch (e) {
      if (notLoadedYet(e) && tries < 14 && !S.fxErrors[id]) { await pause(150); continue; }
      if (!isNoEditor(e) && !notLoadedYet(e)) say(t('c.fx.guiFail', { e: String(e).replace(/^Error:\s*/, '') }));
      return fallback();
    }
  }
}

/** A second click on a plugin whose window is open closes it (FL); otherwise as `showPlugin`. */
export function togglePlugin(id: Id, fallback: () => void) {
  if (editorOpen(id)) return void closeEditor(id);
  void showPlugin(id, fallback);
}
