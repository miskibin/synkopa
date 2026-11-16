// The note tools' one op builder: the scoping rule (selection, else the shown channel),
// the seed parsing, and each verb's fields — the contract the engine side of `transformNotes`
// expects, locked here so the strip and the engine cannot drift apart.
import { describe, expect, it } from 'vitest';
import { buildTransformOp } from './noteTools';

describe('buildTransformOp scoping', () => {
  it('humanize scopes to the selection as sorted indices', () => {
    const op = buildTransformOp('humanize', [0.3, 0.2], '', 7, 64, 3, new Set([5, 2, 9]));
    expect(op.op).toBe('transformNotes');
    expect(op.pattern).toBe(7);
    expect(op.select).toEqual({ kind: 'indices', indices: [2, 5, 9] });
    expect(op.verb).toEqual({ kind: 'humanize', timing: 0.3, velocity: 0.2 });
  });

  it('without a selection the shown channel is the scope — never the whole pattern', () => {
    const op = buildTransformOp('quantize', [1, 1], '', 7, 64, 3, new Set());
    expect(op.select).toEqual({ kind: 'range', from: 0, to: 1e9, ch: 3 });
    expect(op.verb).toEqual({ kind: 'quantize', grid: 1, amount: 1, scaleAware: false });
  });

  it('generators (euclid, drums, melody, progression) stay unscoped: the engine ignores a select', () => {
    expect(buildTransformOp('euclid', [4, 16], '', 7, 64, 3, new Set([1])).select).toBeUndefined();
    expect(buildTransformOp('drums', [4, 16], '', 7, 64, 3, new Set([1])).select).toBeUndefined();
    expect(buildTransformOp('melody', [0.5, 2], '', 7, 64, 3, new Set([1])).select).toBeUndefined();
    expect(buildTransformOp('progression', ['pop', 4, true], '', 7, 64, 3, new Set([1])).select).toBeUndefined();
    expect(buildTransformOp('chord', ['7'], '', 7, 64, 3, new Set([1, 4])).select).toEqual({ kind: 'indices', indices: [1, 4] });
  });
});

describe('buildTransformOp verbs and values', () => {
  it('a numeric field that lost its value (the strip sends 0) does not invent a default', () => {
    const op = buildTransformOp('strum', [0, undefined], '', 7, 64, 3, new Set());
    expect(op.verb).toEqual({ kind: 'strum', width: 0, ramp: 0.05 });
  });

  it('quantize clamps the amount to 0..1 and rejects a zero grid', () => {
    expect(buildTransformOp('quantize', [0, 5], '', 7, 64, 3, new Set()).verb).toMatchObject({ grid: 1, amount: 1 });
    expect(buildTransformOp('quantize', [0.5, -3], '', 7, 64, 3, new Set()).verb).toMatchObject({ grid: 0.5, amount: 0 });
  });

  it('euclid rounds k and n, at least 1 each', () => {
    expect(buildTransformOp('euclid', [2.6, 15.4], '', 7, 64, 3, new Set()).verb).toEqual({ kind: 'euclidFill', ch: 3, k: 3, n: 15, rotate: 0 });
    expect(buildTransformOp('euclid', [0, 0], '', 7, 64, 3, new Set()).verb).toMatchObject({ k: 1, n: 1 });
  });

  it('drums and melody derive their bars from the pattern length', () => {
    expect(buildTransformOp('drums', [4, 16], '', 7, 33, 5, new Set()).verb).toEqual({ kind: 'euclidDrums', voices: [{ ch: 5, k: 4, n: 16 }], bars: 8 });
    expect(buildTransformOp('melody', [0.9, 2], '', 7, 16, 5, new Set()).verb).toMatchObject({ bars: 4, density: 0.9, ch: 5 });
  });

  it('progression takes the template, bars clamped to 1..32, and the voice-leading checkbox', () => {
    expect(buildTransformOp('progression', ['blues', 64, false], '', 7, 64, 3, new Set()).verb).toEqual({ kind: 'progression', ch: 3, bars: 32, template: 'blues', voiceLead: false, degrees: [] });
    expect(buildTransformOp('progression', ['pop', 0], '', 7, 64, 3, new Set()).verb).toMatchObject({ bars: 1, voiceLead: true });
  });

  it('the seed: empty or unusable means none, negatives floor to 0', () => {
    expect(buildTransformOp('humanize', [0.3, 0.2], '  ', 7, 64, 3, new Set()).seed).toBeUndefined();
    expect(buildTransformOp('humanize', [0.3, 0.2], 'abc', 7, 64, 3, new Set()).seed).toBe(0);
    expect(buildTransformOp('humanize', [0.3, 0.2], '-7.9', 7, 64, 3, new Set()).seed).toBe(0);
    expect(buildTransformOp('humanize', [0.3, 0.2], '42', 7, 64, 3, new Set()).seed).toBe(42);
  });
});
