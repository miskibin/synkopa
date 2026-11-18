import { describe, expect, it } from 'vitest';
import { awayZone, releasesFocus, type ZoneEl } from './focusZone';

/** A stand-in element: `cls` are its own classes, `parent` its container. */
function el(tagName: string, cls: string[] = [], parent: ZoneEl | null = null, type?: string): ZoneEl {
  const self: ZoneEl & { cls: string[]; parent: ZoneEl | null } = {
    tagName, cls, parent,
    getAttribute: n => (n === 'type' ? type ?? null : null),
    closest(sel) {
      const want = sel.split(',').map(s => s.trim().replace(/^\./, ''));
      let cur: (ZoneEl & { cls?: string[]; parent?: ZoneEl | null }) | null = self as ZoneEl;
      while (cur) { if (cur.cls?.some(c => want.includes(c))) return cur; cur = cur.parent ?? null; }
      return null;
    },
    contains(other: ZoneEl | null) {
      let cur: (ZoneEl & { parent?: ZoneEl | null }) | null = other;
      while (cur) { if (cur === self) return true; cur = cur.parent ?? null; }
      return false;
    },
  };
  return self;
}

const browser = el('DIV', ['browser']), search = el('INPUT', [], browser, 'search'), sampleRow = el('DIV', ['fitem'], browser);
const playlist = el('CANVAS', ['plgrid']);
const toolbar = el('DIV', ['toolbar']), recBtn = el('BUTTON', [], toolbar), snapSel = el('SELECT', [], toolbar);
const rack = el('DIV', ['win', 'patwin']), rackGrid = el('CANVAS', [], rack);
const fxWin = el('DIV', ['win']), fxKnob = el('DIV', ['dial'], fxWin);
const mixer = el('DIV', ['mixdock']), fader = el('DIV', ['fader'], mixer);
const menu = el('DIV', ['menu']), menuItem = el('BUTTON', [], menu);
const dialogInput = el('INPUT', [], el('DIV', ['settings']), 'number'), dialogLabel = el('SPAN', [], el('DIV', ['settings']));

describe('a click takes the keyboard back (releasesFocus)', () => {
  it('from the Browser search box when the Playlist is clicked, so Space plays instead of typing', () => {
    expect(releasesFocus(search, playlist, false)).toBe(true);
  });
  it('from the Browser list, a toolbar button and a toolbar dropdown', () => {
    expect(releasesFocus(browser, playlist, false)).toBe(true);
    expect(releasesFocus(recBtn, playlist, false)).toBe(true);
    expect(releasesFocus(snapSel, playlist, false)).toBe(true);
  });
  it('from a pattern window when the Playlist or the toolbar is clicked (as before)', () => {
    expect(releasesFocus(rack, playlist, false)).toBe(true);
    expect(releasesFocus(rack, recBtn, false)).toBe(true);
  });
  it('not for a click in the same panel, in the focused element, or with nothing focused', () => {
    expect(releasesFocus(search, sampleRow, false)).toBe(false);
    expect(releasesFocus(rack, rackGrid, false)).toBe(false);
    expect(releasesFocus(snapSel, snapSel, false)).toBe(false);
    expect(releasesFocus(null, playlist, false)).toBe(false);
    expect(releasesFocus(playlist, rackGrid, true)).toBe(false);
  });
  it('not when the click lands in a text field: that takes the focus itself', () => {
    expect(releasesFocus(rack, search, false)).toBe(false);
  });
  it('leaves a dialog field being edited alone when a label beside it is clicked', () => {
    expect(releasesFocus(dialogInput, dialogLabel, false)).toBe(false);
  });
});

describe('clip keys follow the panel you work in (awayZone)', () => {
  it('a press in an effect window, a pattern window or the Mixer takes them away from the Playlist', () => {
    expect(awayZone(fxKnob)).toBe(fxWin);
    expect(awayZone(rackGrid)).toBe(rack);
    expect(awayZone(fader)).toBe(mixer);
  });
  it('a press in the Playlist, the toolbar or the Browser gives them back', () => {
    expect(awayZone(playlist)).toBeNull();
    expect(awayZone(recBtn)).toBeNull();
    // a sample dragged from the Browser lands selected in the Playlist: Delete must reach it
    expect(awayZone(sampleRow)).toBeNull();
  });
  it('a press in a menu changes nothing', () => {
    expect(awayZone(menuItem)).toBeUndefined();
    expect(awayZone(null)).toBeUndefined();
  });
});
