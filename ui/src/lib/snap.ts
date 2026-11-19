import { t } from './i18n';

// Grid snapping. `snap` is the select value: -1 = auto, 0 = off, otherwise beats.
// Named as note values, like the Piano roll's grid: a beat is a quarter note, so 1/4 beat = 1/16.
export const SNAPS: [string, number][] = [
  ['Snap: auto', -1], ['Snap: off', 0], ['Snap: 1/64', 1 / 16], ['Snap: 1/32', 0.125], ['Snap: 1/16', 0.25], ['Snap: 1/12', 1 / 3],
  ['Snap: 1/8', 0.5], ['Snap: 1/4', 1], ['Snap: bar', 4],
];

export function snapName(u: number): string {
  if (u === 1 / 16) return '1/64';
  if (u === 0.125) return '1/32';
  if (u === 0.25) return '1/16';
  if (Math.abs(u - 1 / 3) < 1e-6) return '1/12';
  if (u === 0.5) return '1/8';
  if (u === 1) return '1/4';
  if (u === 2) return '1/2';
  if (u === 4) return 'bar';
  return t('a.snap.offName');
}

/** Smallest of [1/16, 1/8, ¼, ½, 1, 2, 4] beats whose on-screen width is at least 10 px. */
export function autoSnap(pxPerBeat: number): number {
  return [1 / 16, 0.125, 0.25, 0.5, 1, 2, 4].find(u => u * pxPerBeat >= 10) ?? 4;
}

export function snapRes(snap: number, pxPerBeat: number): number {
  return snap < 0 ? autoSnap(pxPerBeat) : snap;
}

export function snapRound(b: number, unit: number): number {
  return unit ? Math.round(b / unit) * unit : Math.round(b * 64) / 64;
}
export function snapFloor(b: number, unit: number): number {
  return unit ? Math.floor(b / unit + 1e-9) * unit : Math.round(b * 64) / 64;
}

export function snapLabel(snap: number, pxPerBeat: number): string {
  return snap < 0 ? 'Snap: auto · ' + snapName(autoSnap(pxPerBeat)) : snap === 0 ? t('a.snap.off') : (SNAPS.find(s => s[1] === snap)?.[0] ?? 'Snap');
}
