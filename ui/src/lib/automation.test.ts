import { describe, expect, it } from 'vitest';
import { sketchProject, mockCatalog } from '../api/mockData';
import { CLIP_GAIN_DB, KEYS_RELEASE, VOL_UNITY, type AutoTarget, type CurveShape, type Channel, type Clip, type Project } from '../api/types';
import { applyAll } from './applyOps';
import { createAutomationOps, curveAt, spansValue, denormalize, normalize, autoValueText, parseAutoValue, readTarget, sortPoints, targetKey, targetLabel, trackForAutomation, valueAt, writeTargetOps } from './automation';
import { newSynthChannel, synthParam } from './synth';
import curvesJson from '../../../crates/engine/tests/fixtures/curves.json?raw';

const cat = mockCatalog();

/** The demo song plus a synth channel, a Synkopa Keys channel and an audio clip to automate. */
function project(): Project {
  const p = sketchProject(cat);
  const synth = { ...newSynthChannel(cat.synth, 7001, null), name: 'Lead' };
  const keys: Channel = { ...p.channels![0], id: 7002, name: 'Piano', kind: 'keys', keys: { release: 350, dynamics: 80 } } as Channel;
  p.channels = [...(p.channels ?? []), synth, keys];
  const src = p.sources[0] ?? { id: 7100, name: 'Loop', path: 'loop.wav', color: '#4f8ef7', dur: 2, sr: 48000, ch: 2, peakDb: -1 };
  if (!p.sources.length) p.sources.push(src as Project['sources'][number]);
  const clip: Clip = { id: 7003, track: 0, start: 0, len: 4, offset: 0, kind: 'audio', ref: src.id, muted: false, gain: -6, fadeIn: 0, fadeOut: 0, pan: 0.5, norm: false, rev: false };
  p.clips.push(clip);
  return p;
}

const targets: AutoTarget[] = [
  { kind: 'chanVol', id: 7001 },
  { kind: 'chanPan', id: 7001 },
  { kind: 'synth', chanId: 7001, param: 'flt_cutoff' },
  { kind: 'keys', chanId: 7002, param: 'release' },
  { kind: 'keys', chanId: 7002, param: 'dynamics' },
  { kind: 'clipGain', id: 7003 },
  { kind: 'clipPan', id: 7003 },
];

describe('automation of channels, synth and Keys parameters, and audio clips', () => {
  it('gives every target its own key', () => {
    const keys = targets.map(targetKey);
    expect(new Set(keys).size).toBe(keys.length);
    // a channel's volume and a mixer insert's volume with the same id are different controls
    expect(targetKey({ kind: 'chanVol', id: 1 })).not.toBe(targetKey({ kind: 'insertVol', id: 1 }));
  });

  it('normalises like the engine (rt.rs apply_automation)', () => {
    const p = project();
    // channel volume is the fader position; pans map −1..1 to 0..1
    expect(normalize(p, cat, { kind: 'chanVol', id: 7001 }, VOL_UNITY)).toBeCloseTo(VOL_UNITY, 9);
    expect(normalize(p, cat, { kind: 'chanPan', id: 7001 }, -1)).toBe(0);
    expect(denormalize(p, cat, { kind: 'clipPan', id: 7003 }, 1)).toBe(1);
    // clip gain is linear in dB over CLIP_GAIN_DB
    expect(denormalize(p, cat, { kind: 'clipGain', id: 7003 }, 0)).toBe(CLIP_GAIN_DB.min);
    expect(denormalize(p, cat, { kind: 'clipGain', id: 7003 }, 1)).toBe(CLIP_GAIN_DB.max);
    expect(normalize(p, cat, { kind: 'clipGain', id: 7003 }, 0)).toBeCloseTo(24 / 36, 9);
    // release is logarithmic: engine `lo * (hi / lo)^v`
    const rel = { kind: 'keys' as const, chanId: 7002, param: 'release' as const };
    expect(denormalize(p, cat, rel, 0.5)).toBeCloseTo(KEYS_RELEASE.min * Math.sqrt(KEYS_RELEASE.max / KEYS_RELEASE.min), 6);
    // a synth parameter uses its catalog range (cutoff is logarithmic)
    const d = synthParam(cat.synth, 'flt_cutoff')!;
    expect(d.log).toBe(true);
    expect(denormalize(p, cat, targets[2], 0)).toBeCloseTo(d.min, 6);
    expect(denormalize(p, cat, targets[2], 1)).toBeCloseTo(d.max, 6);
  });

  it('round-trips every target through its normalisation', () => {
    const p = project();
    for (const t of targets) {
      const x = readTarget(p, t) ?? (t.kind === 'synth' ? synthParam(cat.synth, t.param)!.def : 0);
      expect(denormalize(p, cat, t, normalize(p, cat, t, x)), targetKey(t)).toBeCloseTo(x, 4);
    }
  });

  it('snaps a synth choice to an option', () => {
    const p = project();
    const choice = cat.synth.sections.flatMap(s => s.params).find(d => d.options)!;
    const t: AutoTarget = { kind: 'synth', chanId: 7001, param: choice.id };
    expect(Number.isInteger(denormalize(p, cat, t, 0.37))).toBe(true);
  });

  it('reads the current values, and nothing for a control that is gone', () => {
    const p = project();
    expect(readTarget(p, { kind: 'clipGain', id: 7003 })).toBe(-6);
    expect(readTarget(p, { kind: 'clipPan', id: 7003 })).toBe(0.5);
    expect(readTarget(p, { kind: 'keys', chanId: 7002, param: 'dynamics' })).toBe(80);
    expect(readTarget(p, { kind: 'clipGain', id: 424242 })).toBeNull();
    // a pattern clip has no gain to automate
    const pat = p.clips.find(c => c.kind === 'pat')!;
    expect(readTarget(p, { kind: 'clipGain', id: pat.id })).toBeNull();
    // keys parameters of a channel that is not Synkopa Keys
    expect(readTarget(p, { kind: 'keys', chanId: 7001, param: 'release' })).toBeNull();
  });

  it('starts a new automation at the control\'s current value (a synth default when the patch leaves it out)', () => {
    const p = project();
    let next = 9000;
    const ch = p.channels!.find(c => c.id === 7001)!;
    if (ch.kind === 'synth') delete ch.synth.params.flt_cutoff;
    const r = createAutomationOps(p, cat, targets[2], 'Lead · Cutoff', '#9b7cf8', 0, 16, () => next++);
    const a = applyAll(p, r.ops).automations!.find(x => x.id === r.autoId)!;
    const def = synthParam(cat.synth, 'flt_cutoff')!.def;
    expect(a.points[0].v).toBeCloseTo(normalize(p, cat, targets[2], def), 9);
    const g = createAutomationOps(p, cat, { kind: 'clipGain', id: 7003 }, 'Loop · Clip volume', '#fff', 0, 16, () => next++);
    expect(applyAll(p, g.ops).automations!.find(x => x.id === g.autoId)!.points[0].v).toBeCloseTo(18 / 36, 9);
  });

  it('puts the clip on the first track free over its span, a new bottom track when none is', () => {
    const p = project();
    let next = 9500;
    const clip = (track: number, start: number, len: number) => ({ id: next++, track, start, len, offset: 0, kind: 'pat' as const, ref: 1, muted: false, gain: 0, fadeIn: 0, fadeOut: 0, pan: 0, norm: false, rev: false });
    p.clips = p.clips.filter(c => c.track >= p.tracks.length); // start from an empty playlist
    // every track is busy from bar 7 on (FL-style arrangement); bars 1-6 are free below track 0
    p.clips.push(clip(0, 0, 64));
    for (let t = 1; t < p.tracks.length; t++) p.clips.push(clip(t, 24, 24));
    expect(trackForAutomation(p, 0, 24)).toBe(p.tracks.length > 1 ? 1 : p.tracks.length);
    expect(trackForAutomation(p, 20, 8)).toBe(p.tracks.length); // overlaps everything: a new track
    // an armed track is never picked
    if (p.tracks.length > 1) { p.tracks[1].arm = true; expect(trackForAutomation(p, 0, 24)).toBe(p.tracks.length > 2 ? 2 : p.tracks.length); }
    const r = createAutomationOps(p, cat, targets[2], 'x', '#fff', 20, 8, () => next++);
    expect(r.track).toBe(p.tracks.length);
    expect(applyAll(p, r.ops).tracks.length).toBe(p.tracks.length + 1);
  });

  it('labels each target by what it controls', () => {
    const p = project();
    expect(targetLabel(p, cat, { kind: 'chanVol', id: 7001 })).toMatch(/^Lead · /);
    expect(targetLabel(p, cat, targets[2])).toMatch(/^Lead · /);
    expect(targetLabel(p, cat, { kind: 'keys', chanId: 7002, param: 'release' })).toMatch(/^Piano · /);
    expect(targetLabel(p, cat, { kind: 'clipGain', id: 7003 })).toContain(' · ');
  });
});

describe('segment shapes', () => {
  it('match the engine on the shared reference values', () => {
    const doc = JSON.parse(curvesJson) as { cases: { shape: CurveShape; tension: number; f: number; g: number }[] };
    expect(doc.cases.length).toBeGreaterThan(100);
    for (const c of doc.cases) expect(Math.abs(curveAt(c.shape, c.tension, c.f) - c.g), `${c.shape} ${c.tension} ${c.f}`).toBeLessThan(1e-9);
  });
  it('shape the segment that ends at a point, and old points stay straight', () => {
    expect(valueAt([{ t: 0, v: 0 }, { t: 4, v: 1 }], 1)).toBeCloseTo(0.25, 9);
    expect(valueAt([{ t: 0, v: 0 }, { t: 4, v: 1, shape: 'hold' }], 3.99)).toBe(0);
    expect(valueAt([{ t: 0, v: 0 }, { t: 4, v: 1, shape: 'hold' }], 4)).toBe(1);
    expect(valueAt([{ t: 0, v: 0 }, { t: 4, v: 1, tension: 1 }], 2)!).toBeLessThan(0.05);
  });
  it('keeps the shapes when points are sorted', () => {
    expect(sortPoints([{ t: 4, v: 1, shape: 'stairs', tension: 0.5 }, { t: 0, v: 0 }])).toEqual([{ t: 0, v: 0 }, { t: 4, v: 1, shape: 'stairs', tension: 0.5 }]);
  });
});

describe('point values in the control\'s unit', () => {
  const p = sketchProject(cat);
  it('shows and reads volume in dB and pan as L / R', () => {
    const vol: AutoTarget = { kind: 'insertVol', id: p.inserts[1].id };
    const v = parseAutoValue(p, cat, vol, '-6 dB')!;
    expect(autoValueText(p, cat, vol, v)).toBe('-6.0 dB');
    expect(parseAutoValue(p, cat, vol, '-inf')).toBe(0);
    const pan: AutoTarget = { kind: 'insertPan', id: p.inserts[1].id };
    expect(autoValueText(p, cat, pan, parseAutoValue(p, cat, pan, '25 L')!)).toBe('25L');
    expect(autoValueText(p, cat, pan, 0.5)).toBe('C');
  });
  it('reads kHz and rejects text that is no number', () => {
    const fx = p.inserts.flatMap(m => m.fx).find(f => f.type === 'eq')!;
    const f: AutoTarget = { kind: 'fx', fxId: fx.id, param: 'band:0:f' };
    expect(autoValueText(p, cat, f, parseAutoValue(p, cat, f, '1.2k')!)).toBe('1.20 kHz');
    expect(parseAutoValue(p, cat, f, 'loud')).toBeNull();
  });
});

describe('a target outside its automation clips (engine drive_value)', () => {
  const span = (start: number, len: number, v0: number, v1: number) => ({ start, end: start + len, offset: 0, points: [{ t: 0, v: v0 }, { t: len, v: v1 }] });
  it('holds the first clip\'s first value before it, not the static value', () => {
    const one = [span(16, 8, 0, 1)];
    expect(spansValue(one, 0)).toBe(0);
    expect(spansValue(one, 15.9)).toBe(0);
    expect(spansValue(one, 20)).toBeCloseTo(0.5, 9);
  });
  it('holds where the last clip ended between clips and after the last one', () => {
    const two = [span(2, 2, 0, 0.25), span(8, 2, 1, 0.75)];
    expect(spansValue(two, 0)).toBe(0);
    expect(spansValue(two, 6)).toBe(0.25);
    expect(spansValue(two, 9)).toBeCloseTo(0.875, 9);
    expect(spansValue(two, 12)).toBe(0.75);
  });
  it('is null only without clips', () => {
    expect(spansValue([], 4)).toBeNull();
  });
});

describe('typing a value into a control (writeTargetOps)', () => {
  it('writes the value readTarget reads back, for every target the project holds', () => {
    const p0 = project();
    const ins = p0.inserts[1], fx = p0.inserts.flatMap(m => m.fx)[0];
    const all: AutoTarget[] = [...targets, { kind: 'insertVol', id: ins.id }, { kind: 'insertPan', id: ins.id }, { kind: 'trackVol', id: p0.tracks[0].id }, { kind: 'mix', fxId: fx.id }];
    for (const t of all) {
      const cur = readTarget(p0, t) ?? denormalize(p0, cat, t, 0.5);
      const want = denormalize(p0, cat, t, normalize(p0, cat, t, cur) > 0.5 ? 0.25 : 0.75);
      const ops = writeTargetOps(p0, t, want);
      expect(ops, targetKey(t)).not.toBeNull();
      expect(readTarget(applyAll(p0, ops!), t), targetKey(t)).toBeCloseTo(want, 9);
    }
  });
  it('leaves everything else in the edited object alone', () => {
    const p0 = project(), ins = p0.inserts[1];
    const p1 = applyAll(p0, writeTargetOps(p0, { kind: 'insertVol', id: ins.id }, 0.3)!);
    expect({ ...p1.inserts.find(m => m.id === ins.id)!, vol: ins.vol }).toEqual(ins);
  });
  it('has nothing to write for a hosted plugin parameter, a missing target or a non-number', () => {
    const p0 = project();
    expect(writeTargetOps(p0, { kind: 'plugin', fxId: 1, paramId: 0 } as AutoTarget, 0.5)).toBeNull();
    expect(writeTargetOps(p0, { kind: 'chanVol', id: 999999 }, 0.5)).toBeNull();
    expect(writeTargetOps(p0, { kind: 'chanVol', id: 7001 }, NaN)).toBeNull();
  });
  it('round-trips the text a control shows: typing it back keeps the value', () => {
    const p0 = project();
    for (const t of targets) {
      const v = 0.3, text = autoValueText(p0, cat, t, v).replace(/−/g, '-');
      const back = parseAutoValue(p0, cat, t, text);
      expect(back, targetKey(t) + ' ' + text).not.toBeNull();
      expect(Math.abs(back! - v), targetKey(t) + ' ' + text).toBeLessThan(0.02);
    }
  });
});
