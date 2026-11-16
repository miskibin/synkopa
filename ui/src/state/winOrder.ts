// The stack of floating windows (one order for all of them: `UiState.zTop`), the windows that float
// outside the effect-window layer, and the channel window's place.
import { createSignal } from 'solid-js';
import type { Id } from '../api/types';
import { rackFloating, rollFloating } from './pattern';
import { docked, U } from './ui';

/** The open channel window (synth / WAVE Keys / plugin settings) and its place; null when closed. */
export type Win = { id: Id; x: number; y: number; w: number; z: number };
/** The Mixer floats as a window and is open. */
export const mixFloating = () => !docked('mix') && !U.mixMin;

export const [win, setWin] = createSignal<Win | null>(null);

/**
 * The z-index of a window floating outside the window layer (`.winlayer`, z-index 45): the sample editor, the
 * channel window and the pitch editor. One used after every effect / Channel Rack / Piano roll window sits over
 * the layer, otherwise under it, so clicking a plugin brings the plugin to the front; among themselves the one
 * used last is on top. `z` is the window's place in the stacking order (`UiState.zTop`).
 */
export function outerZ(z: number | undefined): number {
  const layer = U.plugsHidden ? [] : [...U.plugs.map(p => p.z), rackFloating() ? U.rackWin!.z : 0, rollFloating() ? U.rollWin!.z : 0, mixFloating() ? U.mixWin.z : 0];
  const over = (v: number) => layer.every(o => v > o);
  const me = z ?? 0, above = over(me);
  const peers = [U.editor?.z, win()?.z, U.pitchEd?.z].filter((v): v is number => v != null && over(v) === above);
  const lower = peers.filter(v => v < me).length, higher = peers.filter(v => v > me).length;
  return above ? 46 + Math.min(lower, 2) : 44 - Math.min(higher, 2);
}

/** A floating window on screen: what it is, its place in the stacking order, and its id (effect windows). */
export type FloatKind = 'plug' | 'rack' | 'roll' | 'mix' | 'editor' | 'chan' | 'pitch';
export interface Floating { kind: FloatKind; z: number; id?: Id }

/**
 * Every floating window on screen, whatever layer draws it: effect windows, the floating Channel Rack,
 * Piano roll and Mixer, the sample editor, the channel window, the pitch editor. F5 (`plugsHidden`) tucks
 * all of them away; the Mix view hides the pattern windows.
 */
export function shownFloating(): Floating[] {
  if (U.plugsHidden) return [];
  const l: Floating[] = U.plugs.map(p => ({ kind: 'plug' as const, z: p.z, id: p.fxId }));
  if (!U.mixView && rackFloating()) l.push({ kind: 'rack', z: U.rackWin!.z });
  if (!U.mixView && rollFloating()) l.push({ kind: 'roll', z: U.rollWin!.z });
  if (!U.mixView && mixFloating()) l.push({ kind: 'mix', z: U.mixWin.z });
  if (U.editor) l.push({ kind: 'editor', z: U.editor.z ?? 0 });
  const w = win(); if (w) l.push({ kind: 'chan', z: w.z });
  if (U.pitchEd) l.push({ kind: 'pitch', z: U.pitchEd.z ?? 0 });
  return l;
}

/** The floating window in front (the one used last): what Esc closes. */
export function topFloating(): Floating | null {
  let top: Floating | null = null;
  for (const f of shownFloating()) if (!top || f.z > top.z) top = f;
  return top;
}
