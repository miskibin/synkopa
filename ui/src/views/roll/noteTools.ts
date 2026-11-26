// The roll header's note tools (the transform verbs with their declared fields, plus the
// shared seed) and the one place that turns the strip's values into a `transformNotes` op.
// The scoping rule lives here and only here: the selection when there is one, else the
// channel the roll is showing — a pattern is multi-channel, so humanize with the hat shown
// must not jitter the kick. Generators (euclidean, drums, melody, progression) replace their
// whole channel whatever the selection (the engine ignores a select for them), so they stay
// unscoped; `chord` is positional like humanize.
import type { Id, TransformSelect, TransformVerb } from '../../api/types';
import type { TranslationKey } from '../../lib/i18n';
import { PROGRESSION_TEMPLATES } from '../../lib/transforms';

export type ToolId = 'humanize' | 'quantize' | 'euclid' | 'strum' | 'chop' | 'glue' | 'arp' | 'drums' | 'melody' | 'chord' | 'progression';

export type ToolField =
  | { kind: 'num'; label: TranslationKey; def: number; min: number; max: number; step: number }
  | { kind: 'select'; label: TranslationKey; def: string; options: { v: string; l: TranslationKey }[] }
  | { kind: 'check'; label: TranslationKey; def: boolean };

export const HARMONY_OPTIONS: { v: string; l: TranslationKey }[] = [
  { v: 'diatonic', l: 'b.pr.chord.diatonic' },
  ...['maj', 'min', '7', 'maj7', 'min7'].map(id => ({ v: id, l: `b.pr.chord.${id}` as TranslationKey })),
];

export const ARP_OPTIONS: { v: string; l: TranslationKey }[] = [
  { v: 'up', l: 'b.pr.arp.up' }, { v: 'down', l: 'b.pr.arp.down' }, { v: 'upDown', l: 'b.pr.arp.upDown' }, { v: 'random', l: 'b.pr.arp.random' },
];

export const TEMPLATE_OPTIONS: { v: string; l: TranslationKey }[] = [
  ...PROGRESSION_TEMPLATES.map(([id]) => ({ v: id, l: `b.pr.tpl.${id}` as TranslationKey })),
  { v: 'chance', l: 'b.pr.tpl.chance' },
];

/** Each note tool declares its own controls: a number, a select or a checkbox per param. */
export const TOOL_META: Record<ToolId, { label: TranslationKey; icon: string; fields: ToolField[] }> = {
  humanize: { label: 'b.pr.tool.humanize', icon: 'M3 12c2-7 4-7 6 0s4 7 6 0 4-7 6 0', fields: [
    { kind: 'num', label: 'b.pr.tool.humanizeA', def: 0.3, min: 0, max: 1, step: 0.05 }, { kind: 'num', label: 'b.pr.tool.humanizeB', def: 0.2, min: 0, max: 1, step: 0.05 },
  ] },
  quantize: { label: 'b.pr.tool.quantize', icon: 'M5 4v16 M12 4v16 M19 4v16 M5 9h4 M12 15h4', fields: [
    { kind: 'num', label: 'b.pr.tool.quantizeA', def: 1, min: 0.5, max: 8, step: 0.5 }, { kind: 'num', label: 'b.pr.tool.quantizeB', def: 1, min: 0, max: 1, step: 0.05 },
  ] },
  euclid: { label: 'b.pr.tool.euclid', icon: 'M12 4a8 8 0 1 0 0 16a8 8 0 1 0 0-16 M12 4h.01 M19 16h.01 M5 16h.01', fields: [
    { kind: 'num', label: 'b.pr.tool.euclidA', def: 4, min: 1, max: 32, step: 1 }, { kind: 'num', label: 'b.pr.tool.euclidB', def: 16, min: 1, max: 64, step: 1 },
  ] },
  strum: { label: 'b.pr.tool.strum', icon: 'M4 18h7 M8 12h7 M12 6h7', fields: [
    { kind: 'num', label: 'b.pr.tool.strumA', def: 0.5, min: 0, max: 4, step: 0.05 }, { kind: 'num', label: 'b.pr.tool.strumB', def: 0.05, min: -0.5, max: 0.5, step: 0.01 },
  ] },
  chop: { label: 'b.pr.tool.chop', icon: 'M4 12h3 M10.5 12h3 M17 12h3 M8.75 7v10 M15.25 7v10', fields: [
    { kind: 'num', label: 'b.pr.tool.chopA', def: 1, min: 0.25, max: 8, step: 0.25 },
  ] },
  glue: { label: 'b.pr.tool.glue', icon: 'M4 12h16 M8 8l-4 4 4 4 M16 8l4 4-4 4', fields: [] },
  arp: { label: 'b.pr.tool.arp', icon: 'M4 18h3v-3 M9 15h3v-3 M14 12h3V9 M19 9h1', fields: [
    { kind: 'select', label: 'b.pr.tool.arpMode', def: 'up', options: ARP_OPTIONS },
    { kind: 'num', label: 'b.pr.tool.arpRate', def: 1, min: 0.25, max: 8, step: 0.25 },
    { kind: 'num', label: 'b.pr.tool.arpGate', def: 0.9, min: 0.05, max: 1, step: 0.05 },
    { kind: 'num', label: 'b.pr.tool.arpOct', def: 1, min: 1, max: 4, step: 1 },
  ] },
  drums: { label: 'b.pr.tool.drums', icon: 'M4 9a8 3 0 1 0 16 0a8 3 0 1 0-16 0 M4 9v7a8 3 0 0 0 16 0V9', fields: [
    { kind: 'num', label: 'b.pr.tool.drumsA', def: 4, min: 1, max: 32, step: 1 }, { kind: 'num', label: 'b.pr.tool.drumsB', def: 16, min: 1, max: 64, step: 1 },
  ] },
  melody: { label: 'b.pr.tool.melody', icon: 'M9 18V6l10-2v12 M9 18a3 2.5 0 1 1-6 0a3 2.5 0 1 1 6 0 M19 16a3 2.5 0 1 1-6 0a3 2.5 0 1 1 6 0', fields: [
    { kind: 'num', label: 'b.pr.tool.melodyA', def: 0.5, min: 0, max: 1, step: 0.05 }, { kind: 'num', label: 'b.pr.tool.melodyB', def: 2, min: 1, max: 8, step: 1 },
  ] },
  chord: { label: 'b.pr.tool.chord', icon: 'M6 7h12 M6 12h12 M6 17h12', fields: [
    { kind: 'select', label: 'b.pr.tool.chordH', def: 'diatonic', options: HARMONY_OPTIONS },
  ] },
  progression: { label: 'b.pr.tool.progression', icon: 'M4 8h3 M4 12h3 M4 16h3 M10.5 6h3 M10.5 10h3 M10.5 14h3 M17 9h3 M17 13h3 M17 17h3', fields: [
    { kind: 'select', label: 'b.pr.tool.progTpl', def: 'pop', options: TEMPLATE_OPTIONS },
    { kind: 'num', label: 'b.pr.tool.progBars', def: 4, min: 1, max: 32, step: 1 },
    { kind: 'check', label: 'b.pr.tool.progVl', def: true },
  ] },
};

export interface TransformOp {
  op: 'transformNotes';
  pattern: Id;
  verb: TransformVerb;
  seed?: number;
  select?: TransformSelect;
}

/** The strip's values as one `transformNotes` op. An empty (or unusable) seed is left to the
 * engine; a numeric field that lost its value takes the tool's default back. */
export function buildTransformOp(
  id: ToolId, vals: readonly unknown[], seedStr: string, patId: Id, patLen: number, bar: number, chan: Id, sel: ReadonlySet<number>,
): TransformOp {
  const num = (i: number, d: number) => (typeof vals[i] === 'number' ? (vals[i] as number) : d);
  const str = (i: number) => String(vals[i] ?? '');
  const verb: TransformVerb = id === 'humanize' ? { kind: 'humanize', timing: num(0, 0.3), velocity: num(1, 0.2) }
    : id === 'quantize' ? { kind: 'quantize', grid: num(0, 1) || 1, amount: Math.min(1, Math.max(0, num(1, 1))), scaleAware: false }
    : id === 'euclid' ? { kind: 'euclidFill', ch: chan, k: Math.max(1, Math.round(num(0, 4))), n: Math.max(1, Math.round(num(1, 16))), rotate: 0 }
    : id === 'strum' ? { kind: 'strum', width: num(0, 0.5), ramp: num(1, 0.05) }
    : id === 'drums' ? { kind: 'euclidDrums', voices: [{ ch: chan, k: Math.max(1, Math.round(num(0, 4))), n: Math.max(1, Math.round(num(1, 16))) }], bars: Math.max(1, Math.round(patLen / bar)) }
    : id === 'melody' ? { kind: 'melody', ch: chan, bars: Math.max(1, Math.round(patLen / bar)), density: Math.min(1, Math.max(0.05, num(0, 0.5))), repeat: Math.max(1, Math.round(num(1, 2))) }
    : id === 'chord' ? { kind: 'chord', harmony: str(0) }
    : id === 'chop' ? { kind: 'chop', len: Math.max(0.25, num(0, 1)) }
    : id === 'glue' ? { kind: 'glue' }
    : id === 'arp' ? { kind: 'arpeggiate', mode: (['up', 'down', 'upDown', 'random'].includes(str(0)) ? str(0) : 'up') as 'up', rate: Math.max(0.25, num(1, 1)),
      gate: Math.min(1, Math.max(0.05, num(2, 0.9))), octaves: Math.min(4, Math.max(1, Math.round(num(3, 1)))) }
    : { kind: 'progression', ch: chan, bars: Math.min(32, Math.max(1, Math.round(num(1, 4)))), template: str(0), voiceLead: vals[2] !== false, degrees: [] };
  const seed = seedStr.trim() === '' ? undefined : Math.max(0, Math.floor(+seedStr || 0));
  return {
    op: 'transformNotes', pattern: patId, verb,
    ...(seed !== undefined ? { seed } : {}),
    ...(id === 'humanize' || id === 'quantize' || id === 'strum' || id === 'chord' || id === 'chop' || id === 'glue' || id === 'arp'
      ? sel.size > 0
        ? { select: { kind: 'indices' as const, indices: Array.from(sel).sort((x, y) => x - y) } }
        : { select: { kind: 'range' as const, from: 0, to: 1e9, ch: chan } }
      : {}),
  };
}
