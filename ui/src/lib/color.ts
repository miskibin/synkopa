// sRGB colour mixing (what the prototype did with CSS color-mix), cached.
const cache = new Map<string, string>();

function parse(hex: string): [number, number, number] {
  let h = hex.replace('#', '');
  if (h.length === 3) h = h.split('').map(c => c + c).join('');
  const n = parseInt(h.slice(0, 6), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** `pct` percent of `a` mixed with `b`. */
export function mix(a: string, pct: number, b: string): string {
  const k = a + pct + b;
  let v = cache.get(k);
  if (v) return v;
  const x = parse(a), y = parse(b), t = pct / 100;
  v = 'rgb(' + x.map((c, i) => Math.round(c * t + y[i] * (1 - t))).join(',') + ')';
  cache.set(k, v);
  return v;
}

export function alpha(hex: string, a: number): string {
  const [r, g, b] = parse(hex);
  return `rgba(${r},${g},${b},${a})`;
}

/**
 * Colours new channels cycle through: spread around the hue wheel (blue, amber, green, pink …) so
 * neighbouring channels, their steps and their piano-roll notes are told apart at a glance. Every
 * entry reads on both dark and light surfaces.
 */
export const CHANNEL_PALETTE = [
  '#4f8ef7', '#e0a44a', '#5fbf8a', '#d9678f', '#9a8cf0', '#47b8a8',
  '#f07f4f', '#c77ddb', '#a3d65f', '#6cc4ff', '#e8c55a', '#e05263',
];
/** The colour menu's swatches: the channel palette, the older blues/teals (so existing colours stay pickable) and a grey. */
export const SWATCHES = [...CHANNEL_PALETTE, '#5aa9e6', '#69c3d6', '#7c8cf8', '#8b93a1'];

/**
 * The colour for a new channel, given the colours already in use (rack order): the least used
 * palette colour, ties broken by palette order, never the same as the last channel's colour.
 */
export function nextChannelColor(used: string[]): string {
  const count = new Map<string, number>();
  for (const c of used) count.set(c.toLowerCase(), (count.get(c.toLowerCase()) ?? 0) + 1);
  const last = used.length ? used[used.length - 1].toLowerCase() : null;
  let best = CHANNEL_PALETTE[0], bestN = Infinity;
  for (const c of CHANNEL_PALETTE) {
    if (c === last) continue;
    const n = count.get(c) ?? 0;
    if (n < bestN) { best = c; bestN = n; }
  }
  return best;
}
