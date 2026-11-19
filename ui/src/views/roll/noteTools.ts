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
  | { kind: 'num'; label: TranslationKey; def: number; min: number; max: number; step: number }
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
