import { t as tl } from '../lib/i18n';
// Automation in the UI: the right-click menu every automatable control shares, creating an
// automation clip, and the live (automated) value a control shows while the song plays.
// Also capture (issue #201): while the transport records automation, control tweaks during
// playback write envelope points instead of static values, and a tweak on an automated
// control overrides its envelope until re-enabled (or the transport stops).
import { createMemo, createRoot } from 'solid-js';
import { createSignal } from '../lib/reactive';
import type { AutoPoint, AutoTarget, Automation, Id, Op } from '../api/types';
import { automationSpans, autoValueText, createAutomationOps, denormalize, normalize, parseAutoValue, readTarget, spansValue, targetKey, targetLabel, trackForAutomation, writeTargetOps } from '../lib/automation';
import { fmtPos } from '../lib/format';
import { newClip } from '../lib/clipOps';
import { addFrameTask } from '../ui/frameLoop';
import { commit, enqueue, Gesture, wheelEdit } from './edit';
import { be } from '../api/backend';
import { allocId, rawOf, S } from './store';
import { curBeat } from './telemetry';
import { openMenu, openRename, revealInPlaylist, say, setU, U } from './ui';

const [beatSig, setBeatSig] = createSignal(0);
/**
 * Automation clips per target key, by start. Built once per edit; a knob then looks up only its
 * own few clips instead of walking every clip each frame (65 mixer strips × 1400 clips held
 * playback at 20 fps). Overridden automations (a tweak won while playing) drive nothing, like
 * the engine's timeline leaves them out.
 */
const index = createRoot(() => createMemo(() => automationSpans({ ...S.project, automations: (S.project.automations ?? []).filter(a => !S.overriddenAutos.includes(a.id)) })));
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
      const ops = writeTargetOps(rawOf(S.project), target, denormalize(S.project, S.catalog, target, n));
      if (!ops) return;
      commit(ops, label);
      say(label + ' · ' + autoValueText(S.project, S.catalog, target, n));
    }, label);
  };
  const ovr = overriddenIds(target);
  openMenu(x, y, [
    { t: 'title', label },
    { t: 'item', label: tl('a.au.automate'), kbd: S.project.loop ? tl('a.au.loop') : tl('a.au.bars4'), act: () => automate(target, color, opts) },
    ...(auto ? [{ t: 'item' as const, label: tl('a.au.selClips'), act: () => setU('sel', clips().map(c => c.id)) }] : []),
    ...(ovr.length ? [{ t: 'item' as const, label: tl('a.au.reenable'), title: tl('a.au.reenableTip'), act: () => reEnableAutomation(target) }] : []),
    { t: 'sep' },
    { t: 'item', label: tl('a.au.typeIn'), disabled: !canType || auto, title: auto ? tl('a.au.typeAuto') : !canType ? tl('a.au.typePlugin') : '', act: typeIn },
    ...(reset ? [{ t: 'item' as const, label: tl('a.au.reset'), kbd: tl('a.au.altClick'), act: reset }] : []),
  ], 170);
}

// ---- override (a manual tweak on an automated control, while the song plays) ----

const reportOverrideError = (e: unknown) => {
  console.warn('automation override failed:', e);
  say(tl('a.err', { e: String((e as Error)?.message ?? e) }));
};

function automationIdsOf(target: AutoTarget): Id[] {
  const k = targetKey(target);
  return (S.project.automations ?? []).filter(a => targetKey(a.target) === k).map(a => a.id);
}

/** A manual tweak outside the drag helpers (a hosted plugin row writes its parameter directly):
 * while playing, mark the target's automations overridden so the tweak is the one that counts.
 * Repeats while the request is in flight are harmless (the engine's set insert is idempotent);
 * once the state event shows the override, this stays quiet until it is re-enabled. */
export function noteManualTweak(target: AutoTarget) {
  if (!U.playing) return;
  for (const id of automationIdsOf(target)) {
    if (!S.overriddenAutos.includes(id)) void enqueue(() => be().automationOverride(id, true)).catch(reportOverrideError);
  }
}

/** The target's automations that a tweak overrode while playing (the knob shows the manual
 * value; the envelope rests until re-enabled or the transport stops, like Ableton's). */
export function overriddenIds(target: AutoTarget): Id[] {
  const k = targetKey(target);
  return (S.project.automations ?? []).filter(a => targetKey(a.target) === k && S.overriddenAutos.includes(a.id)).map(a => a.id);
}

/** "Re-enable automation": the envelope drives the control again (the static value the tweak
 * wrote stays underneath, unheard while the envelope plays). */
export function reEnableAutomation(target: AutoTarget) {
  for (const id of overriddenIds(target)) void enqueue(() => be().automationOverride(id, false)).catch(reportOverrideError);
}

/** A tweak won over this control's envelope while playing (the knob shows the manual value). */
export const isOverridden = (target: AutoTarget) => overriddenIds(target).length > 0;

// ---- capture (the record filter's "automation"): tweaks become envelope points ----

/** Capture decimation bound: an envelope a tweak records into never grows past this many
 * points (two minutes of constant wiggling stays well under it; the line stays editable). */
export const AUTO_CAPTURE_MAX = 2000;

/** Injectable playhead for tests: capture time comes from the transport beat. */
let beatNow: () => number = curBeat;
/** Test hook: capture at a scripted playhead. Returns the previous source. */
export function setBeatSourceForTests(fn: (() => number) | null): () => number {
  const prev = beatNow;
  beatNow = fn ?? curBeat;
  return prev;
}

/**
 * The points a control's tweak records: decimated (a point is kept on a direction change or a
 * ≥1% move — straight stretches of the drag stay one segment), the same beat updates in place,
 * the cap stops growth. `t` is envelope-local (clip-relative), as the engine stores it.
 */
export class CaptureBuf {
  private lt = -1;
  private lv = 0;
  private ld = 0; // the last move's direction: straight-on ramps are skipped, reversals kept
  capped = false;
  constructor(readonly points: AutoPoint[]) {}
  /** Add `v` at `t`; true when a point was written. `force` (a drag's release value) skips the decimation. */
  add(t: number, v: number, force = false): boolean {
    v = Math.min(1, Math.max(0, v));
    const last = this.points[this.points.length - 1];
    if (last && Math.abs(t - last.t) < 1e-6) {
      last.v = v;
      this.lv = v;
      return true;
    }
    if (!force && this.lt >= 0 && t > this.lt && Math.sign(v - this.lv) === this.ld && Math.abs(v - this.lv) < 0.01) return false;
    if (this.points.length >= AUTO_CAPTURE_MAX) {
      if (!this.capped) say(tl('a.au.recFull', { n: AUTO_CAPTURE_MAX }));
      this.capped = true;
      return false;
    }
    this.points.push({ t: Math.max(0, t), v });
    this.ld = Math.sign(v - this.lv);
    this.lv = v;
    this.lt = t;
    return true;
  }
}

/** True when a control tweak records an envelope: the record filter's automation is armed and
 * the song plays (a pattern preview's clock is not the playlist's; there a tweak writes static). */
export const captureArmed = () => S.transport.rec && S.transport.recAutomation && U.playing;

const autoClip = (id: Id, ref: Id, track: number, start: number, len: number) => newClip(id, track, start, len, 'auto', ref);

/** What one control's tweak captures into: its automation and the clip that drives it at the
 * capture beat — the one under the playhead (the latest-starting one where they overlap),
 * else the latest that already ended — the same choice the engine's `drive_value` makes. */
function captureTargetOf(target: AutoTarget): { auto: Automation; clip: { id: Id; start: number; len: number; offset: number } | null } | null {
  const autos = (S.project.automations ?? []).filter(a => targetKey(a.target) === targetKey(target));
  if (!autos.length) return null;
  const beat = beatNow();
  const clips = S.project.clips.filter(c => c.kind === 'auto' && !c.muted && autos.some(a => a.id === c.ref));
  const covering = clips.filter(c => c.start <= beat && beat < c.start + c.len).sort((a, b) => a.start - b.start);
  const ended = clips.filter(c => c.start + c.len <= beat).sort((a, b) => a.start + a.len - (b.start + b.len));
  const clip = covering.at(-1) ?? ended.at(-1);
  return { auto: (clip && autos.find(a => a.id === clip.ref)) || autos[0], clip: clip ? { id: clip.id, start: clip.start, len: clip.len, offset: clip.offset } : null };
}

/** What a control tweak does between press and release: capture envelope points while the
 * record filter's automation is armed (and the song plays), otherwise the static values —
 * which override an automated control's envelope while playing. Both are one undo step.
 * `mkStatic` builds the ops the plain tweak writes from the normalised value. */
export interface Tweak {
  move(norm: number): void;
  end(): void;
}

/** The plain path: a `Gesture` of static ops, plus the override when the control is automated. */
class StaticTweak implements Tweak {
  private g: Gesture | null = null;
  constructor(target: AutoTarget | null, label: string, private mk: (norm: number) => Op[] | null) {
    this.g = new Gesture(label);
    // a fader over an envelope wins until re-enabled or stopped (Ableton's override)
    if (target) noteManualTweak(target);
  }
  move(norm: number) {
    const ops = this.mk(norm);
    if (ops) this.g?.update(ops);
  }
  end() {
    this.g?.end();
    this.g = null;
  }
}

class CaptureTweak implements Tweak {
  private g: Gesture;
  private buf: CaptureBuf;
  private auto: Automation;
  private local: (beat: number) => number; // song beat → envelope-local t (the clip's)
  private held: number | null = null;
  private lastTick = 0;
  private clip: { id: Id; start: number; len: number; offset: number } | null = null;
  constructor(target: AutoTarget, label: string, private key: string, private color: string, private opts?: { current?: number; paramLabel?: string }) {
    latches.get(key)?.kill(); // an older latch on this control stops writing before this drag does
    latches.delete(key);
    this.g = new Gesture(label);
    const ct = captureTargetOf(target);
    if (ct && ct.clip) {
      this.auto = structuredClone(rawOf(ct.auto));
      this.clip = { ...ct.clip };
      this.local = beat => beat - this.clip!.start + this.clip!.offset;
    } else {
      const lp = S.project.loop;
      const start = lp ? lp.a : Math.max(0, U.start), len = lp ? lp.b - lp.a : 16;
      if (ct) {
        // the envelope exists but nothing plays it (its clip was deleted or muted):
        // give the recorded points a clip again
        this.auto = structuredClone(rawOf(ct.auto));
        const id = allocId();
        this.g.update([{ op: 'putClips', clips: [autoClip(id, this.auto.id, trackForAutomation(rawOf(S.project), start, len), start, len)] }]);
        this.clip = { id, start, len, offset: 0 };
        this.local = beat => beat - start;
      } else {
        // no envelope yet: create it with its clip, like the right-click "Automate" — same undo step
        const name = targetLabel(S.project, S.catalog, target, this.opts?.paramLabel);
        const r = createAutomationOps(rawOf(S.project), S.catalog, target, name, this.color || '#e0a44a', start, len, allocId, this.opts?.current);
        this.g.update(r.ops);
        this.auto = structuredClone((r.ops.find(o => o.op === 'putAutomation') as { automation: Automation } | undefined)!.automation);
        const clip = (r.ops.find(o => o.op === 'putClips') as { clips: { id: Id; start: number; len: number }[] } | undefined)?.clips[0];
        this.clip = clip ? { id: clip.id, start: clip.start, len: clip.len, offset: 0 } : null;
        this.local = clip ? beat => beat - clip.start : beat => beat;
        say(tl('a.au.recStart', { name }));
      }
    }
    this.buf = new CaptureBuf(this.auto.points);
  }
  move(norm: number) {
    this.held = norm;
    if (this.buf.add(this.local(beatNow()), norm)) this.send();
  }
  end() {
    if (this.held != null && this.buf.add(this.local(beatNow()), this.held, true)) this.send();
    if (S.transport.recAutoLatch && captureArmed()) startLatch(this.key, this);
    else this.close();
  }
  /** The latch tail: keep writing the held value until the transport stops or the filter disarms. */
  tick(now: number): boolean {
    if (this.held == null || !captureArmed()) {
      this.close();
      return false;
    }
    if (now - this.lastTick < 120) return true;
    this.lastTick = now;
    if (this.buf.add(this.local(beatNow()), this.held, true)) this.send();
    return true;
  }
  /** A new tweak on the same control replaces a latch (and its open gesture). */
  kill() {
    this.close();
  }
  private close() {
    this.g.end();
  }
  private send() {
    const ops: Op[] = [{ op: 'putAutomation', automation: { ...this.auto, points: [...this.buf.points] } }];
    // a point past the clip's end would never play: grow the clip to cover the recording
    const maxT = this.buf.points.reduce((m, p) => Math.max(m, p.t), 0);
    if (this.clip && maxT > this.clip.len) {
      this.clip.len = maxT + 1;
      const c = S.project.clips.find(q => q.id === this.clip!.id);
      ops.push({ op: 'putClips', clips: [c ? { ...rawOf(c), len: this.clip.len } : autoClip(this.clip.id, this.auto.id, 0, this.clip.start, this.clip.len)] });
    }
    this.g.update(ops);
  }
}

/** Open latch tails per target key (Ableton: latched controls hold until the transport stops). */
const latches = new Map<string, CaptureTweak>();
let latchStop: (() => void) | null = null;
function startLatch(key: string, l: CaptureTweak) {
  latches.get(key)?.kill();
  latches.set(key, l);
  if (latchStop) return;
  latchStop = addFrameTask(now => {
    for (const [k, l] of [...latches]) if (!l.tick(now)) latches.delete(k);
    if (!latches.size) {
      latchStop?.();
      latchStop = null;
    }
  });
}

/** Begin a control tweak (a drag): call in the knob's `onStart`; feed `onChange`/`onEnd` to
 * `move`/`end`. Capture or static is decided here, once. `target` is null for a control that
 * cannot be automated (the plain path only). */
export function autoTweak(target: AutoTarget | null, label: string, mkStatic: (norm: number) => Op[] | null, opts?: { current?: number; paramLabel?: string; color?: string }): Tweak {
  if (captureArmed() && target) return new CaptureTweak(target, label, targetKey(target), opts?.color ?? '#e0a44a', opts);
  return new StaticTweak(target, label, mkStatic);
}

const wheelCaps = new Map<string, { t: CaptureTweak; at: number }>();
let wheelTimer: ReturnType<typeof setTimeout> | null = null;
/** Overrides already asked for (a wheel ticks many times before the state event comes back). */
/** Begin a control tweak from a wheel tick: like `autoTweak`, but the capture gesture closes
 * after the wheel goes quiet (450 ms), the way `wheelEdit` closes its gestures. */
export function autoWheel(target: AutoTarget | null, key: string, label: string, norm: number, mkStatic: (norm: number) => Op[] | null, opts?: { current?: number; paramLabel?: string; color?: string }): void {
  if (captureArmed() && target) {
    let w = wheelCaps.get(key);
    if (!w) {
      w = { t: new CaptureTweak(target, label, targetKey(target), opts?.color ?? '#e0a44a', opts), at: 0 };
      wheelCaps.set(key, w);
    }
    w.t.move(norm);
    w.at = performance.now();
    if (wheelTimer) clearTimeout(wheelTimer);
    wheelTimer = setTimeout(() => {
      for (const [k, w] of [...wheelCaps]) {
        if (performance.now() - w.at >= 450) {
          w.t.end();
          wheelCaps.delete(k);
        }
      }
      wheelTimer = null;
    }, 460);
    return;
  }
  if (target) noteManualTweak(target);
  const ops = mkStatic(norm);
  if (ops) wheelEdit(key, label, ops);
}
