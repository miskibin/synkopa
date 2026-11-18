import { t as tl } from '../lib/i18n';
// Automation in the UI: the right-click menu every automatable control shares, creating an
// automation clip, and the live (automated) value a control shows while the song plays.
import { createMemo, createRoot, createSignal } from 'solid-js';
import { unwrap } from 'solid-js/store';
import type { AutoTarget } from '../api/types';
import { automationSpans, autoValueText, createAutomationOps, denormalize, normalize, parseAutoValue, readTarget, spansValue, targetKey, targetLabel, writeTargetOps } from '../lib/automation';
import { fmtPos } from '../lib/format';
import { addFrameTask } from '../ui/frameLoop';
import { commit } from './edit';
import { allocId, S } from './store';
import { curBeat } from './telemetry';
import { openMenu, openRename, revealInPlaylist, say, setU, U } from './ui';

const [beatSig, setBeatSig] = createSignal(0);
/**
 * Automation clips per target key, by start. Built once per edit; a knob then looks up only its
 * own few clips instead of walking every clip each frame (65 mixer strips × 1400 clips held
 * playback at 20 fps).
 */
const index = createRoot(() => createMemo(() => automationSpans(S.project)));
const keys = () => index();
const valueFor = (t: AutoTarget, beat: number) => spansValue(index().get(targetKey(t)) ?? [], beat);

// While playing, publish the playhead ~30×/s — only controls that are automated subscribe to it.
let lastSet = 0;
addFrameTask(now => {
  if (!U.playing || !keys().size) return;
  if (now - lastSet < 30) return;
  lastSet = now; setBeatSig(curBeat());
});

/** Is this control driven by an automation clip? */
export const isAutomated = (t: AutoTarget) => keys().has(targetKey(t));

/**
 * The value (plain units) a control should display: the automated value at the playhead while
 * playing (or at the start marker when stopped) — also before, between and after its clips, like
 * the engine plays it — or null when not automated.
 */
export function autoDisplay(t: AutoTarget): number | null {
  if (!isAutomated(t)) return null;
  const beat = U.playing ? beatSig() : U.start;
  const v = valueFor(t, beat);
  return v == null ? null : denormalize(S.project, S.catalog, t, v);
}

/** Create an automation clip for `target` at the start marker (or over the loop), 4 bars long.
 * `current` is a hosted plugin parameter's live normalized value (the project does not know it);
 * `paramLabel` names the parameter ("Decay") for the clip and mixer lane. */
export function automate(target: AutoTarget, color = '#e0a44a', opts?: { current?: number; paramLabel?: string }) {
  const lp = S.project.loop;
  const start = lp ? lp.a : Math.max(0, U.start), len = lp ? lp.b - lp.a : 16;
  const name = targetLabel(S.project, S.catalog, target, opts?.paramLabel);
  const r = createAutomationOps(S.project, S.catalog, target, name, color, start, len, allocId, opts?.current);
  commit(r.ops, tl('a.au.label'));
  setU('sel', [r.clipId]);
  // the clip lands on the first empty track or a new one at the bottom, often off screen
  revealInPlaylist(r.track, start);
  say(tl('a.au.created', { name, n: r.track + 1, pos: fmtPos(start) }));
}

/**
 * The right-click menu of a control that cannot be automated (a clip's fades, a channel's root
 * key): the same menu, with "Automate" shown disabled and why.
 */
export function controlMenu(x: number, y: number, label: string, reset: () => void, why: string) {
  openMenu(x, y, [
    { t: 'title', label },
    { t: 'item', label: tl('a.au.automate'), disabled: true, title: why, act: () => {} },
    { t: 'sep' },
    { t: 'item', label: tl('a.au.reset'), kbd: tl('a.au.altClick'), act: reset },
  ], 140);
}

/** The shared right-click menu of an automatable control. `opts` carries a hosted
 * plugin parameter's live value and name (see `automate`). */
export function paramMenu(x: number, y: number, target: AutoTarget, reset: (() => void) | null, color?: string, opts?: { current?: number; paramLabel?: string }) {
  const auto = isAutomated(target);
  const clips = () => S.project.clips.filter(c => c.kind === 'auto' && S.project.automations?.find(a => a.id === c.ref && targetKey(a.target) === targetKey(target)));
  const label = targetLabel(S.project, S.catalog, target, opts?.paramLabel);
  // typing needs the value in the project (a hosted plugin keeps its own) and a control that is not driven by a clip
  const cur = readTarget(S.project, target);
  // … and a value whose text reads back as itself: a choice ("Off"), a beat fraction ("3/4 beat") or a
  // centred pan ("C") would be parsed as something else, or not at all. An audio clip's gain and pan
  // are shared by every clip of its sample (the knob sees to that), which one clip's op would break.
  const shown = cur == null ? null : normalize(S.project, S.catalog, target, cur);
  const back = shown == null ? null : parseAutoValue(S.project, S.catalog, target, autoValueText(S.project, S.catalog, target, shown).replace(/−/g, '-'));
  const canType = cur != null && target.kind !== 'clipGain' && target.kind !== 'clipPan'
    && back != null && Math.abs(back - shown!) < 0.02 && writeTargetOps(S.project, target, cur) != null;
  const typeIn = () => {
    const now = readTarget(S.project, target); if (now == null) return;
    const text = autoValueText(S.project, S.catalog, target, normalize(S.project, S.catalog, target, now));
    openRename(x, y, text, v => {
      const n = parseAutoValue(S.project, S.catalog, target, v.replace(/−/g, '-'));
      if (n == null) { say(tl('a.au.typeBad', { v })); return; }
      // plain objects: ops are cloned on their way to the engine, which a store proxy does not survive
      const ops = writeTargetOps(unwrap(S.project), target, denormalize(S.project, S.catalog, target, n));
      if (!ops) return;
      commit(ops, label);
      say(label + ' · ' + autoValueText(S.project, S.catalog, target, n));
    }, label);
  };
  openMenu(x, y, [
    { t: 'title', label },
    { t: 'item', label: tl('a.au.automate'), kbd: S.project.loop ? tl('a.au.loop') : tl('a.au.bars4'), act: () => automate(target, color, opts) },
    ...(auto ? [{ t: 'item' as const, label: tl('a.au.selClips'), act: () => setU('sel', clips().map(c => c.id)) }] : []),
    { t: 'sep' },
    { t: 'item', label: tl('a.au.typeIn'), disabled: !canType || auto, title: auto ? tl('a.au.typeAuto') : !canType ? tl('a.au.typePlugin') : '', act: typeIn },
    ...(reset ? [{ t: 'item' as const, label: tl('a.au.reset'), kbd: tl('a.au.altClick'), act: reset }] : []),
  ], 170);
}
