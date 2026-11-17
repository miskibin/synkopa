import { describe, expect, it } from 'vitest';
import { CHANNEL_PALETTE, nextChannelColor, SWATCHES } from './color';

describe('nextChannelColor', () => {
  it('starts at the first palette colour', () => {
    expect(nextChannelColor([])).toBe(CHANNEL_PALETTE[0]);
  });
  it('walks the palette as channels are added, so neighbours differ', () => {
    const used: string[] = [];
    for (let i = 0; i < CHANNEL_PALETTE.length; i++) used.push(nextChannelColor(used));
    expect(used).toEqual(CHANNEL_PALETTE);
  });
  it('prefers the least used colour once the palette wraps', () => {
    const used = [...CHANNEL_PALETTE, CHANNEL_PALETTE[0], CHANNEL_PALETTE[1]];
    expect(nextChannelColor(used)).toBe(CHANNEL_PALETTE[2]);
  });
  it('never repeats the last channel’s colour, even when that one is least used', () => {
    // every colour used twice except the last channel's, which is used once
    const used = [...CHANNEL_PALETTE.slice(1), ...CHANNEL_PALETTE.slice(1), CHANNEL_PALETTE[0]];
    expect(nextChannelColor(used)).not.toBe(CHANNEL_PALETTE[0]);
  });
  it('treats colours case-insensitively (old projects may store upper-case hex)', () => {
    expect(nextChannelColor(['#4F8EF7'])).toBe(CHANNEL_PALETTE[1]);
  });
  it('keeps every palette colour pickable from the colour menu', () => {
    for (const c of CHANNEL_PALETTE) expect(SWATCHES).toContain(c);
    expect(new Set(SWATCHES).size).toBe(SWATCHES.length);
  });
});
