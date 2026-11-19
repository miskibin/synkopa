// Alternate shading of a time grid (Playlist, Piano roll): every other unit is a shade lighter,
// so odd and even beats or bars are told apart without counting lines. The unit follows the zoom:
// beats while a beat is wide enough to read as a band, then bars, then groups of 4 and 16 bars.

/** Narrowest band, in px, that still reads as a band and not as a thick line. */
export const SHADE_MIN_PX = 22;

/** Length in beats of one shaded band at `pxPerBeat` (4/4: a bar is 4 beats). `minPx` is the narrowest
 *  band allowed: an arrangement is read in bars, so the Playlist asks for wider bands than the Piano roll. */
export function shadeUnit(pxPerBeat: number, minPx = SHADE_MIN_PX): number {
  for (const u of [1, 4, 16, 64]) if (u * pxPerBeat >= minPx) return u;
  return 256;
}

/** The shaded bands [start, length] in beats that touch `from..to`: the second of every pair of units. */
export function shadeBands(from: number, to: number, unit: number): [number, number][] {
  const out: [number, number][] = [];
  for (let b = Math.floor(from / (2 * unit)) * 2 * unit + unit; b < to; b += 2 * unit) if (b + unit > from) out.push([b, unit]);
  return out;
}
