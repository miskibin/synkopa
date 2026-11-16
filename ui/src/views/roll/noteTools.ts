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

export type ToolId = 'humanize' | 'quantize' | 'euclid' | 'strum' | 'drums' | 'melody' | 'chord' | 'progression';

export type ToolField =
  | { kind: 'num'; label: TranslationKey; def: number }
  | { kind: 'select'; label: TranslationKey; def: string; options: { v: string; l: TranslationKey }[] }
  | { kind: 'check'; label: TranslationKey; def: boolean };

export const HARMONY_OPTIONS: { v: string; l: TranslationKey }[] = [
  { v: 'diatonic', l: 'b.pr.chord.diatonic' },
  ...['maj', 'min', '7', 'maj7', 'min7'].map(id => ({ v: id, l: `b.pr.chord.${id}` as TranslationKey })),
];

export const TEMPLATE_OPTIONS: { v: string; l: TranslationKey }[] = [
  ...PROGRESSION_TEMPLATES.map(([id]) => ({ v: id, l: `b.pr.tpl.${id}` as TranslationKey })),
  { v: 'chance', l: 'b.pr.tpl.chance' },
];

/** Each note tool declares its own controls: a number, a select or a checkbox per param. */
export const TOOL_META: Record<ToolId, { label: TranslationKey; fields: ToolField[] }> = {
  humanize: { label: 'b.pr.tool.humanize', fields: [
    { kind: 'num', label: 'b.pr.tool.humanizeA', def: 0.3 }, { kind: 'num', label: 'b.pr.tool.humanizeB', def: 0.2 },
  ] },
  quantize: { label: 'b.pr.tool.quantize', fields: [
    { kind: 'num', label: 'b.pr.tool.quantizeA', def: 1 }, { kind: 'num', label: 'b.pr.tool.quantizeB', def: 1 },
  ] },
  euclid: { label: 'b.pr.tool.euclid', fields: [
    { kind: 'num', label: 'b.pr.tool.euclidA', def: 4 }, { kind: 'num', label: 'b.pr.tool.euclidB', def: 16 },
  ] },
  strum: { label: 'b.pr.tool.strum', fields: [
    { kind: 'num', label: 'b.pr.tool.strumA', def: 0.5 }, { kind: 'num', label: 'b.pr.tool.strumB', def: 0.05 },
  ] },
  drums: { label: 'b.pr.tool.drums', fields: [
    { kind: 'num', label: 'b.pr.tool.drumsA', def: 4 }, { kind: 'num', label: 'b.pr.tool.drumsB', def: 16 },
  ] },
  melody: { label: 'b.pr.tool.melody', fields: [
    { kind: 'num', label: 'b.pr.tool.melodyA', def: 0.5 }, { kind: 'num', label: 'b.pr.tool.melodyB', def: 2 },
  ] },
  chord: { label: 'b.pr.tool.chord', fields: [
    { kind: 'select', label: 'b.pr.tool.chordH', def: 'diatonic', options: HARMONY_OPTIONS },
  ] },
  progression: { label: 'b.pr.tool.progression', fields: [
    { kind: 'select', label: 'b.pr.tool.progTpl', def: 'pop', options: TEMPLATE_OPTIONS },
    { kind: 'num', label: 'b.pr.tool.progBars', def: 4 },
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
  id: ToolId, vals: readonly unknown[], seedStr: string, patId: Id, patLen: number, chan: Id, sel: ReadonlySet<number>,
): TransformOp {
  const num = (i: number, d: number) => (typeof vals[i] === 'number' ? (vals[i] as number) : d);
  const str = (i: number) => String(vals[i] ?? '');
  const verb: TransformVerb = id === 'humanize' ? { kind: 'humanize', timing: num(0, 0.3), velocity: num(1, 0.2) }
    : id === 'quantize' ? { kind: 'quantize', grid: num(0, 1) || 1, amount: Math.min(1, Math.max(0, num(1, 1))), scaleAware: false }
    : id === 'euclid' ? { kind: 'euclidFill', ch: chan, k: Math.max(1, Math.round(num(0, 4))), n: Math.max(1, Math.round(num(1, 16))), rotate: 0 }
    : id === 'strum' ? { kind: 'strum', width: num(0, 0.5), ramp: num(1, 0.05) }
    : id === 'drums' ? { kind: 'euclidDrums', voices: [{ ch: chan, k: Math.max(1, Math.round(num(0, 4))), n: Math.max(1, Math.round(num(1, 16))) }], bars: Math.max(1, Math.round(patLen / 4)) }
    : id === 'melody' ? { kind: 'melody', ch: chan, bars: Math.max(1, Math.round(patLen / 4)), density: Math.min(1, Math.max(0.05, num(0, 0.5))), repeat: Math.max(1, Math.round(num(1, 2))) }
    : id === 'chord' ? { kind: 'chord', harmony: str(0) }
    : { kind: 'progression', ch: chan, bars: Math.min(32, Math.max(1, Math.round(num(1, 4)))), template: str(0), voiceLead: vals[2] !== false, degrees: [] };
  const seed = seedStr.trim() === '' ? undefined : Math.max(0, Math.floor(+seedStr || 0));
  return {
    op: 'transformNotes', pattern: patId, verb,
    ...(seed !== undefined ? { seed } : {}),
    ...(id === 'humanize' || id === 'quantize' || id === 'strum' || id === 'chord'
      ? sel.size > 0
        ? { select: { kind: 'indices' as const, indices: Array.from(sel).sort((x, y) => x - y) } }
        : { select: { kind: 'range' as const, from: 0, to: 1e9, ch: chan } }
      : {}),
  };
}
