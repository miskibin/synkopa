// Error boundaries for the app's surfaces (Solid 2's `<Errored>`).
//
// In Solid 2 an error thrown while rendering — in a JSX binding, a component body or a memo —
// halts the whole reactive system for good: every later update is ignored and the app freezes
// (`REACTIVITY_HALTED`; Solid 1 threw from the write and kept working). `lib/reactive`'s effect
// helpers already catch their own errors; these catch the rest, one surface each. The failed
// surface says what happened and offers to build it again; its window frame (title bar, close,
// dock) stays usable wherever the frame is outside the boundary, and every other view keeps
// working. The error is still reported as uncaught (console, the window's error handler, a test
// run), as an effect's error is.
//
// One variant per kind of surface, all with the same message:
// - `PanelBoundary`: a panel's content (the panel's own frame stays outside). In a window, "Try
//   again" rebuilds the window's content with the component that owns it (`WindowRebuild`).
// - `WindowBoundary`: a whole window, frame included; the failure keeps a minimal frame (its name,
//   move, close) where the window was, so it can still be moved and closed.
// - `DialogBoundary`: a dialog; the failure is a small dialog that takes no focus hostage and
//   closes with ×, Close, Esc or a press outside.
// - `PopupBoundary`: a menu or a list popup; a small box where it was.
// - `ChipBoundary`: a part of a bar (the toolbar); one line, the details on hover.
// - `AppBoundary`: the whole window, for a failure no surface caught.
// A failed surface comes back by itself when what it read changes (e.g. the edit that broke it is
// undone), and "Try again" builds it afresh from the current state: its parts that did not fail are
// disposed and made again too (a part may hold a bad value in its own state, set by an effect that
// is held back while the failure shows). Closing a failed window or dialog disposes it with its
// failure.
import { createContext, Errored, Show, untrack, useContext } from 'solid-js';
import type { JSX } from '@solidjs/web';
import { captureOn, createSignal, watch } from '../lib/reactive';
import { t } from '../lib/i18n';
import { moveWindow } from './drag';
import { modal } from './modal';

/** Report a caught error the way an uncaught one is, without throwing into Solid. */
function report(e: unknown): void {
  queueMicrotask(() => { throw e; });
}

function message(e: unknown): string {
  const m = e instanceof Error ? e.message : String(e);
  return m.length > 300 ? m.slice(0, 300) + '…' : m;
}

/**
 * The rebuild of the window a panel is in (a `WindowBoundary`'s content), null outside a window.
 * A window's component keeps what it attaches to its content (size observers, listeners on its
 * refs) outside the panel's boundary, with the title bar: rebuilding the content alone would
 * leave those on the removed elements, so a panel in a window retries by rebuilding the window's
 * content, its component included.
 */
const WindowRebuild = createContext<(() => void) | null>(null);

/**
 * `children` behind Solid's `<Errored>`: the error is reported, `fallback` shows it, and its
 * `retry` builds `children` again from scratch (then clears the failure, if the new build holds).
 * `window`: nested panels retry through this guard. `inWindow`: retry through the window's, if any.
 */
function Guard(props: { fallback: (err: () => unknown, retry: () => void) => JSX.Element; children: JSX.Element; window?: boolean; inWindow?: boolean }) {
  const [build, setBuild] = createSignal(0);
  const rebuild = () => setBuild(build() + 1);
  const isWindow = untrack(() => props.window);
  const outer = untrack(() => props.inWindow) ? useContext(WindowRebuild) : null;
  return (
    <Errored fallback={(err, reset) => {
      // each error reported once; the fallback built once (a second error only updates its text, the focus stays)
      watch(err, e => report(e));
      return untrack(() => props.fallback(err, () => { if (outer) outer(); else { rebuild(); reset(); } }));
    }}>
      <Show when={build() + 1} keyed>{_build => (isWindow ? <WindowRebuild value={rebuild}>{props.children}</WindowRebuild> : props.children)}</Show>
    </Errored>
  );
}

/** What a failed surface says: what stopped and that the project is not affected, the error, Try again (and Close where it closes). */
function Failure(props: { name: string; err: () => unknown; reset: () => void; close?: () => void; dialog?: boolean }) {
  return (
    <div class="panelerr" role="alert">
      <b>{t('d.ui.panelFailed', { name: props.name })}</b>
      <span>{t(props.dialog ? 'd.ui.dialogFailed.body' : 'd.ui.panelFailed.body')}</span>
      <code class="mono" title={message(props.err())}>{message(props.err())}</code>
      <div class="acts">
        <button class="smallbtn primary" onClick={() => props.reset()}>{t('d.ui.panelFailed.retry')}</button>
        <Show when={props.close}>{close => <button class="smallbtn" onClick={() => close()()}>{t('d.ui.panelFailed.close')}</button>}</Show>
      </div>
    </div>
  );
}

/**
 * `children` rendered behind an error boundary. `name` is the panel as the user knows it
 * ("Mixer"). After a failure, "Try again" builds the content again from the current state.
 */
export function PanelBoundary(props: { name: string; children: JSX.Element }) {
  return (
    <Guard inWindow fallback={(err, reset) => {
      return <Failure name={props.name} err={err} reset={reset} />;
    }}>
      {props.children}
    </Guard>
  );
}

/** Where a floating window is (its `h` when the window has a fixed height). */
export interface WinGeom { x: number; y: number; w: number; h?: number }

/**
 * A whole window behind an error boundary, for a failure in its frame or its component body: the
 * failure shows in a minimal frame of the window's size and stacking, which still moves (by its
 * title bar), comes to the front and closes. `docked`: laid out in the dock like the window.
 */
export function WindowBoundary(props: {
  name: string; close: () => void; closeTip?: string;
  /** Classes the window's own frame has (`patwin rollwin`): its layout in the dock and its size limits. */
  class?: string; docked?: boolean;
  geom?: () => WinGeom | null | undefined; move?: (x: number, y: number) => void; z?: number; front?: () => void;
  children: JSX.Element;
}) {
  return (
    <Guard window fallback={(err, reset) => {
      const g = () => (props.docked ? undefined : props.geom?.());
      return (
        <div ref={captureOn('pointerdown', () => { if (!props.docked) props.front?.(); })} tabindex={0}
          class={['win failwin ' + (props.class ?? ''), { docked: !!props.docked }]}
          style={g() ? { left: g()!.x + 'px', top: g()!.y + 'px', width: g()!.w + 'px', height: g()!.h != null ? g()!.h + 'px' : undefined, 'z-index': props.z } : undefined}>
          <div class="winhd" onPointerDown={e => { const m = props.move; if (g() && m) moveWindow(e, () => g()!, m); }}>
            <span class="title">{props.name}</span>
            <div style={{ flex: 1 }} />
            <button class="closebtn" title={props.closeTip ?? t('d.ui.panelFailed.close')} aria-label={props.closeTip ?? t('d.ui.panelFailed.close')} onClick={() => props.close()}>×</button>
          </div>
          <Failure name={props.name} err={err} reset={reset} />
        </div>
      );
    }}>
      {props.children}
    </Guard>
  );
}

/**
 * Keys in a failed dialog. Like the dialog it stands for, it keeps them from the app behind it
 * (Delete would remove the selected clips, Space would start playback instead of pressing the
 * focused button). Esc closes it; save, undo and redo still reach the app: the message says saving
 * works, and undoing the edit that broke the dialog brings it back.
 */
function dialogKey(e: KeyboardEvent, close: () => void): void {
  if (e.key === 'Escape') { e.stopPropagation(); e.preventDefault(); close(); return; }
  if (savesOrUndoes(e) || e.key === 'Tab') return;
  e.stopPropagation();
}
function savesOrUndoes(e: KeyboardEvent): boolean {
  const k = e.key.toLowerCase();
  return (e.ctrlKey || e.metaKey) && !e.altKey && (k === 's' || k === 'z' || k === 'y');
}

/**
 * A dialog behind an error boundary. Put it inside the dialog's open condition: closing disposes
 * the failure with the dialog. The failure is a small dialog that takes the keyboard while it is
 * shown (`dialogKey`, `modal`) like any other dialog: Tab stays in it, a press outside closes it.
 */
export function DialogBoundary(props: { name: string; close: () => void; z?: number; children: JSX.Element }) {
  return (
    <Guard fallback={(err, reset) => {
      return (
        <div class="overlay" style={props.z != null ? { 'z-index': props.z } : undefined} onPointerDown={() => props.close()}>
          <div ref={el => modal(el, { close: () => props.close(), passKeys: savesOrUndoes })} tabindex={-1}
            class="dialog faildlg" role="alertdialog" aria-label={props.name}
            onPointerDown={e => e.stopPropagation()} onKeyDown={e => dialogKey(e, props.close)}>
            <div class="hd"><span>{props.name}</span><button title={t('d.ui.panelFailed.close')} aria-label={t('d.ui.panelFailed.close')} onClick={() => props.close()}>×</button></div>
            <Failure name={props.name} err={err} reset={reset} close={() => props.close()} dialog />
          </div>
        </div>
      );
    }}>
      {props.children}
    </Guard>
  );
}

/**
 * A menu or list popup behind an error boundary (inside its open condition, like a dialog's): the
 * failure is a small box at the popup's place, in its frame (`class`: `ctxmenu` keeps a press in
 * it from closing the menu), with Try again and Close.
 */
export function PopupBoundary(props: { name: string; x: number; y: number; close: () => void; class?: string; children: JSX.Element }) {
  return (
    <Guard fallback={(err, reset) => {
      return (
        <div class={'menu ' + (props.class ?? '')} style={{ left: Math.max(4, Math.min(props.x, window.innerWidth - 324)) + 'px', top: Math.max(4, props.y) + 'px' }}
          onPointerDown={e => e.stopPropagation()}>
          <div class="panelerr small" role="alert">
            <b>{t('d.ui.partFailed', { name: props.name })}</b>
            <code class="mono" title={message(err())}>{message(err())}</code>
            <div class="acts">
              <button class="smallbtn primary" onClick={reset}>{t('d.ui.panelFailed.retry')}</button>
              <button class="smallbtn" onClick={() => props.close()}>{t('d.ui.panelFailed.close')}</button>
            </div>
          </div>
        </div>
      );
    }}>
      {props.children}
    </Guard>
  );
}

/** A part of a bar behind an error boundary: one line in its place ("Transport: error", Try again), the error on hover. */
export function ChipBoundary(props: { name: string; children: JSX.Element }) {
  return (
    <Guard fallback={(err, reset) => {
      return (
        <span class="panelerr chip" role="alert" title={t('d.ui.partFailed.tip', { name: props.name, err: message(err()) })}>
          <b>{t('d.ui.partFailed', { name: props.name })}</b>
          <button class="smallbtn" onClick={reset}>{t('d.ui.panelFailed.retry')}</button>
        </span>
      );
    }}>
      {props.children}
    </Guard>
  );
}

/**
 * The whole app behind an error boundary, for what no surface caught (the app's own frame, a
 * surface's open condition): the window says so instead of freezing. The engine holds the project,
 * its undo and unsaved changes, so a reload of the window loses nothing of it.
 */
export function AppBoundary(props: { children: JSX.Element }) {
  return (
    <Guard fallback={(err, reset) => {
      return (
        <div class="panelerr apperr" role="alert">
          <b>{t('d.ui.appFailed')}</b>
          <span>{t('d.ui.appFailed.body')}</span>
          <code class="mono" title={message(err())}>{message(err())}</code>
          <div class="acts">
            <button class="smallbtn primary" onClick={reset}>{t('d.ui.panelFailed.retry')}</button>
            <button class="smallbtn" onClick={() => location.reload()}>{t('d.ui.appFailed.reload')}</button>
          </div>
        </div>
      );
    }}>
      {props.children}
    </Guard>
  );
}
