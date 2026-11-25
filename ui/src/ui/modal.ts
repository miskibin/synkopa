// The one keyboard contract every modal dialog follows (Welcome, Unsaved changes, Settings, Export,
// the pickers, …): register the dialog's element with `modal(el, { close })` from its `ref`.
//
// - Tab / Shift+Tab cycle through the dialog's controls and never reach the app behind it.
// - Esc closes the dialog on top (when it may be closed: `close` given), wherever the focus is, unless
//   a control inside used Esc itself first (a list, an edited name: they call preventDefault or
//   stopPropagation) — then that control's Esc wins and the dialog stays.
// - The app's shortcuts (Space, tools, F-keys, the typing keyboard) stay off while a modal is open
//   (`modalOwnsKey`, checked by App.tsx's key handler).
// - Focus goes into the dialog when it opens (`data-autofocus`, else the dialog itself) and back to
//   what had it before (the opener) when it closes.
// - Stacked dialogs: the one drawn on top (highest z-index, then the newest) owns the keyboard.
//
// Why not <dialog>.showModal(): it puts the dialog in the browser's top layer, above everything the
// app draws itself — the themed select lists (SelectPopup) and context menus that open from a
// dialog would then sit under it and be inert. WebView2 and WebKitGTK both support it, but a trap
// that keeps the app's own stacking works the same in both and needs no dependency.
//
// The element is registered from a `ref`, which Solid 2 runs without an owner (no onCleanup), so a
// dialog's end is noticed when its element leaves the document (a MutationObserver that runs only
// while a modal is open).

import { closeMenu } from '../state/ui';

export interface ModalOpts {
  /** Esc (and Esc pressed while the focus somehow sits outside it) closes it. Omitted: Esc does nothing. */
  close?: () => void;
  /** Keys that still reach the app's shortcuts while this dialog is on top (a failure dialog lets Save / Undo through). */
  passKeys?: (e: KeyboardEvent) => boolean;
}

interface Entry extends ModalOpts {
  el: HTMLElement;
  /** What had the focus when the dialog opened: it gets it back on close. */
  opener: HTMLElement | null;
  /** The last control focused inside: the focus comes back here if something pulls it out. */
  last: HTMLElement | null;
  seq: number;
  /** In the document at least once (a ref runs before the element is inserted). */
  seen: boolean;
  /** Its layer's z-index, read once it is in the document (a dialog's layer does not change). */
  z?: number;
}

let stack: Entry[] = [];
let seq = 0;
let observer: MutationObserver | null = null;

/** Popups that open from a dialog but are drawn outside it (themed select lists, context menus). */
const POPUP = '.selpop, .ctxmenu';
const inPopup = (n: EventTarget | null) => n instanceof Element && !!n.closest(POPUP);
/** Is the event's target inside the dialog (a key sent to `window` has no node)? */
const inside = (t: Entry, n: EventTarget | null) => n instanceof Node && t.el.contains(n);
const popupOpen = () => !!document.querySelector('.ctxmenu');

const FOCUSABLE = 'a[href], button, input:not([type="hidden"]), select, textarea, [tabindex], [contenteditable="true"], audio[controls], video[controls], summary';

function visible(el: HTMLElement): boolean {
  if (el.closest('[hidden], [inert]')) return false;
  const check = (el as HTMLElement & { checkVisibility?: (o?: object) => boolean }).checkVisibility;
  return typeof check === 'function' ? check.call(el, { visibilityProperty: true }) : true;
}

/** The dialog's Tab stops in order (one per radio group, like the browser's own Tab). */
export function tabStops(root: HTMLElement): HTMLElement[] {
  const all = Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(el =>
    el.tabIndex >= 0 && !(el as HTMLButtonElement).disabled && visible(el));
  return all.filter(el => {
    if (!(el instanceof HTMLInputElement) || el.type !== 'radio' || !el.name) return true;
    const group = all.filter((o): o is HTMLInputElement => o instanceof HTMLInputElement && o.type === 'radio' && o.name === el.name);
    const checked = group.find(o => o.checked);
    return checked ? checked === el : group[0] === el;
  });
}

/** z-index of the dialog's layer (the highest one among its ancestors). */
function zOf(el: HTMLElement): number {
  let z = 0;
  for (let n: HTMLElement | null = el; n && n !== document.body; n = n.parentElement) {
    const v = parseInt(getComputedStyle(n).zIndex, 10);
    if (!Number.isNaN(v) && v > z) z = v;
  }
  return z;
}

/** Of two dialogs on one layer, is `a` drawn over `b`? The later one in the document is; the newer one when either has left it. */
function later(a: Entry, b: Entry): boolean {
  if (a.el.isConnected && b.el.isConnected) return !!(b.el.compareDocumentPosition(a.el) & Node.DOCUMENT_POSITION_FOLLOWING);
  return a.seq > b.seq;
}

/** The dialog that owns the keyboard among `of`: drawn on top (z-index, then document order). */
function topOf(of: Entry[]): Entry | null {
  let best: Entry | null = null;
  for (const e of of) {
    if (e.z == null) {
      if (!e.el.isConnected) continue;
      e.z = zOf(e.el);
    }
    if (!best || e.z > best.z! || (e.z === best.z && later(e, best))) best = e;
  }
  return best;
}
const top = () => topOf(stack.filter(e => e.el.isConnected));

/** Is a modal dialog open? */
export const modalOpen = (): boolean => top() != null;

/** Does an open modal keep this key from the app's own shortcuts? */
export function modalOwnsKey(e: KeyboardEvent): boolean {
  const t = top();
  return !!t && !t.passKeys?.(e);
}

function initialFocus(e: Entry) {
  const a = document.activeElement;
  if (a && e.el.contains(a) && a !== e.el) return;
  const want = Array.from(e.el.querySelectorAll<HTMLElement>('[data-autofocus]')).find(x => visible(x) && !(x as HTMLButtonElement).disabled);
  (want ?? e.el).focus({ preventScroll: true });
}

function focusIn(e: Entry, where: 'first' | 'last' | 'resume') {
  if (where === 'resume') {
    if (e.last && e.last.isConnected && e.el.contains(e.last) && visible(e.last)) return e.last.focus({ preventScroll: true });
    return initialFocus(e);
  }
  const s = tabStops(e.el);
  ((where === 'first' ? s[0] : s[s.length - 1]) ?? e.el).focus({ preventScroll: true });
}

function escape(t: Entry, e: KeyboardEvent) {
  // a context menu opened from the dialog closes first (it listens to the app's Esc otherwise)
  if (popupOpen()) {
    e.preventDefault();
    closeMenu();
    return;
  }
  if (!t.close) return;
  e.preventDefault();
  t.close();
}

function tab(t: Entry, e: KeyboardEvent) {
  const s = tabStops(t.el), a = document.activeElement as HTMLElement | null;
  if (!s.length) { e.preventDefault(); t.el.focus({ preventScroll: true }); return; }
  const i = a ? s.indexOf(a) : -1;
  // inside, between the ends: the browser's own order is right
  if (i > 0 && i < s.length - 1) return;
  if (i === 0 && !e.shiftKey && s.length > 1) return;
  if (i === s.length - 1 && e.shiftKey && s.length > 1) return;
  // at an end (or not on a stop): wrap round
  e.preventDefault();
  s[e.shiftKey ? s.length - 1 : 0].focus({ preventScroll: true });
}

/** Keys aimed outside the top dialog (the focus on the page body, or somewhere it should not be). */
function onKeyCapture(e: KeyboardEvent) {
  const t = top();
  if (!t || inPopup(e.target) || inside(t, e.target)) return;
  if (e.key === 'Tab') { e.preventDefault(); e.stopPropagation(); focusIn(t, e.shiftKey ? 'last' : 'first'); return; }
  if (e.key === 'Escape') { e.stopPropagation(); escape(t, e); }
}

/** Keys inside the top dialog, after its own controls had them (Solid handles keys on `document` too, registered first). */
function onKeyBubble(e: KeyboardEvent) {
  const t = top();
  if (!t || !inside(t, e.target)) return;
  if (e.key === 'Tab' && !e.defaultPrevented && !e.altKey && !e.ctrlKey && !e.metaKey) return tab(t, e);
  if (e.key === 'Escape' && !e.defaultPrevented && !e.cancelBubble) escape(t, e);
}

/** Focus that moves out of the top dialog (a click can't: the veil covers the app; code can) comes back. */
function onFocusIn(e: FocusEvent) {
  const t = top();
  if (!t) return;
  const n = e.target;
  if (inside(t, n)) { if (n !== t.el) t.last = n as HTMLElement; return; }
  if (inPopup(n)) return;
  focusIn(t, 'resume');
}

/** A dialog's element left the document: it closed. Give the focus back. */
function prune() {
  const gone: Entry[] = [];
  for (const e of stack) {
    if (e.el.isConnected) e.seen = true;
    else if (e.seen) gone.push(e);
  }
  if (!gone.length) return;
  const wasTop = topOf(stack);
  stack = stack.filter(e => !gone.includes(e));
  for (const g of gone) {
    // a dialog opened while this one closed (the palette running "Settings"): its opener was this
    // one's, or nothing — it hands the focus back to where this one would have
    for (const e of stack) {
      if (e.seq > g.seq && (!e.opener || !e.opener.isConnected || g.el.contains(e.opener) || e.opener === document.body)) e.opener = g.opener;
    }
  }
  const now = top();
  const a = document.activeElement;
  // the action that closed it may have put the focus somewhere on purpose (an editor it opened)
  const free = !a || a === document.body || gone.some(g => g.el.contains(a));
  if (now) {
    if (now !== wasTop || free || !now.el.contains(a)) {
      const g = gone.find(x => x.opener && now.el.contains(x.opener) && x.opener.isConnected);
      if (g) g.opener!.focus({ preventScroll: true });
      else focusIn(now, 'resume');
    }
  } else if (free) {
    const g = [...gone].sort((x, y) => y.seq - x.seq).find(x => x.opener && x.opener.isConnected && x.opener !== document.body && visible(x.opener));
    g?.opener!.focus({ preventScroll: true });
  }
  if (!stack.length) stopWatching();
}

function startWatching() {
  if (observer || typeof MutationObserver === 'undefined') return;
  observer = new MutationObserver(prune);
  observer.observe(document.body, { childList: true, subtree: true });
}
function stopWatching() {
  observer?.disconnect();
  observer = null;
}

/**
 * Make `el` (the dialog box, not its veil) a modal dialog. Call it from the element's `ref`.
 * Sets role="dialog" (unless it has a role), aria-modal and tabindex=-1 (so a press on its text
 * keeps the focus in it).
 */
export function modal(el: HTMLElement, opts: ModalOpts = {}): void {
  install();
  if (!el.hasAttribute('role')) el.setAttribute('role', 'dialog');
  el.setAttribute('aria-modal', 'true');
  if (!el.hasAttribute('tabindex')) el.tabIndex = -1;
  const a = document.activeElement as HTMLElement | null;
  const entry: Entry = { el, ...opts, opener: a && a !== document.body ? a : null, last: null, seq: ++seq, seen: false };
  stack.push(entry);
  startWatching();
  let tries = 0;
  const settle = () => {
    if (!el.isConnected) { if (++tries < 20) setTimeout(settle, 0); else stack = stack.filter(e => e !== entry); return; }
    entry.seen = true;
    if (top() === entry) initialFocus(entry);
  };
  queueMicrotask(settle);
}

let installed = false;
/** The listeners (once; `modal` calls it, main.tsx calls it at start so the order is fixed). */
export function install() {
  if (installed || typeof window === 'undefined') return;
  installed = true;
  window.addEventListener('keydown', onKeyCapture, true);
  document.addEventListener('keydown', onKeyBubble);
  document.addEventListener('focusin', onFocusIn, true);
}

/** Tests: forget every dialog. */
export function resetModals() {
  stack = [];
  stopWatching();
}
