import type { IconDefinition } from '@fortawesome/free-solid-svg-icons';
import { factoryFile } from '../lib/factory';
import { t, type TranslationKey } from '../lib/i18n';
import {
  faDrum, faGuitar, faHeadphones, faMicrophone, faMusic, faRecordVinyl, faWandMagicSparkles, faWaveSquare,
} from '@fortawesome/free-solid-svg-icons';

/** Filled instrument glyph: viewBox + path (Font Awesome Free, CC BY 4.0, or our own 24×24 drawings). */
type Glyph = { vb: string; d: string; label: string; eo?: boolean };

const fa = (i: IconDefinition, label: TranslationKey): Glyph => {
  const [w, h, , , d] = i.icon;
  return { vb: `0 0 ${w} ${h}`, d: Array.isArray(d) ? d.join(' ') : d, get label() { return t(label); } };
};

/** Own drawings where Font Awesome only has Pro icons (piano, bass guitar). */
const PIANO = 'M4 9.5C4 5.91 6.91 3 10.5 3C14.09 3 17 5.91 17 9.5C17 10.74 17.78 11.83 18.96 12.22C20.78 12.83 22 14.54 22 16.46V18C22 19.66 20.66 21 19 21H7C5.34 21 4 19.66 4 18V9.5ZM6.5 14.5H7.75V17C7.75 17.41 8.09 17.75 8.5 17.75C8.91 17.75 9.25 17.41 9.25 17V14.5H10.25V17C10.25 17.41 10.59 17.75 11 17.75C11.41 17.75 11.75 17.41 11.75 17V14.5H12.75V17C12.75 17.41 13.09 17.75 13.5 17.75C13.91 17.75 14.25 17.41 14.25 17V14.5H16.25V17C16.25 17.41 16.59 17.75 17 17.75C17.41 17.75 17.75 17.41 17.75 17V14.5H19.5V18.5H6.5V14.5Z';
const BASS = 'M20.2 1.8C20.9 1.1 22.1 1.1 22.8 1.8C23.4 2.4 23.5 3.3 23 4.1L20.6 7.3C20.1 8 19.2 8.2 18.5 7.8L13.8 12.5C15.1 12.4 16.3 13.2 16.7 14.4C17.2 15.8 16.6 17.3 15.5 18.4L13.4 20.5C11.3 22.6 7.9 22.6 5.8 20.5L4.5 19.2C2.4 17.1 2.4 13.7 4.5 11.6L7.6 8.5C8.7 7.4 10.4 7 11.7 7.7C12.5 8.1 12.9 9 12.6 9.8C12.3 10.5 11.5 10.8 10.8 10.5C10.3 10.3 9.7 10.4 9.3 10.8L8.8 11.3C9.6 11.8 10.6 11.8 11.4 11.2L17.2 5.4C16.8 4.7 17 3.8 17.6 3.2L19.1 1.8H20.2ZM8.3 13.6C8.6 13.3 9.1 13.3 9.4 13.6L11.4 15.6C11.7 15.9 11.7 16.4 11.4 16.7C11.1 17 10.6 17 10.3 16.7L8.3 14.7C8 14.4 8 13.9 8.3 13.6ZM6.2 15.7C6.5 15.4 7 15.4 7.3 15.7L9.3 17.7C9.6 18 9.6 18.5 9.3 18.8C9 19.1 8.5 19.1 8.2 18.8L6.2 16.8C5.9 16.5 5.9 16 6.2 15.7Z';

/** Track icons in picker order. Ids are saved in the project, never rename them. */
export const INST_ICONS: Record<string, Glyph> = {
  vocal: fa(faMicrophone, 'b.ii.vocal'),
  guitar: fa(faGuitar, 'b.ii.guitar'),
  bass: { vb: '0 0 24 24', d: BASS, get label() { return t('b.ii.bass'); }, eo: true },
  piano: { vb: '0 0 24 24', d: PIANO, get label() { return t('b.ii.piano'); }, eo: true },
  drums: fa(faDrum, 'b.ii.drums'),
  synth: fa(faWaveSquare, 'b.ii.synth'),
  sample: fa(faRecordVinyl, 'b.ii.sample'),
  fx: fa(faWandMagicSparkles, 'b.ii.fx'),
  bus: fa(faHeadphones, 'b.ii.bus'),
  other: fa(faMusic, 'b.ii.other'),
};

export function InstIcon(props: { id?: string | null; size?: number }) {
  const g = () => (props.id ? INST_ICONS[props.id] : undefined);
  return (
    <>{g() && (
      <svg class="insticon" width={props.size ?? 14} height={props.size ?? 14} viewBox={g()!.vb} fill="currentColor" aria-label={g()!.label}>
        <path fill-rule={g()!.eo ? 'evenodd' : 'nonzero'} d={g()!.d} />
      </svg>
    )}</>
  );
}

// ---------------------------------------------------------------- automatic icons
// Shown whenever the user has not picked one: a guess from the name, else from what plays.

const RULES: [RegExp, string][] = [
  // "rap" and "chór" only as words: "Kick Trap" is a drum, "Chord" a synth
  [/vo[ck]|wokal|g[łl]os|\brap\b|\bch[óo]r(?!d)|adlib|\bmic|\bvox|\bvoice|zwrotk|refren|nagran/i, 'vocal'],
  // drum words before bass: "808 Kick" is a drum, "808 Bass" a bass
  [/kick|snare|hat|clap|perc|drum|b[ęe]b|beat|\btoms?\b|cymb|\bride\b|crash|shaker|\brim|stopa|werbel|talerz|conga|bongo|cajon|cowbell|tambo|clave|maraca|\bsnap\b|stick/i, 'drums'],
  [/bass|\bbas\b|\bbasy?\b|808|\bsub\b/i, 'bass'],
  [/piano|pian|keys|rhodes|klaw|organ|organy/i, 'piano'],
  [/guit|gitar/i, 'guitar'],
  [/synth|syntez|lead|\bpad|pluck|arp|chord|akord|string|smycz|bell/i, 'synth'],
  [/\bfx\b|riser|sweep|impact|noise|efekt|transition|whoosh/i, 'fx'],
  [/loop|sampl|vinyl|chop|p[ęe]tl/i, 'sample'],
  [/\bbus\b|send|grup|odsłuch|reverb|delay|pog[łl]os/i, 'bus'],
];

/** Icon id guessed from a name, or null. */
export function guessIcon(name: string | null | undefined): string | null {
  if (!name) return null;
  for (const [re, id] of RULES) if (re.test(name)) return id;
  return null;
}

/** A track's icon: the picked one, else a guess from its name. */
export const trackIcon = (t: { icon?: string | null; name: string }): string | null => t.icon ?? guessIcon(t.name);

/** What each standard-library group is: the library already knows, the name need not be read. */
const LIB_GROUP_ICON: Record<string, string> = {
  Kicks: 'drums', Snares: 'drums', Claps: 'drums', Hats: 'drums', Cymbals: 'drums', Toms: 'drums', Percussion: 'drums',
  808: 'bass', FX: 'fx',
};
/** The icon of a standard-library file's group, when the group says what the file is (not Tonal, Orchestra, Loops). */
const libIcon = (path: string | null | undefined): string | null => {
  const f = path ? factoryFile(path) : undefined;
  return (f && LIB_GROUP_ICON[f.group]) || null;
};

/** A sample's icon: its standard-library group (Kicks, 808…) when it is one of the library's files, else a guess from its name. */
export function sampleIcon(s: { name: string; path?: string | null }): string | null {
  return libIcon(s.path) ?? guessIcon(s.name);
}

type ChannelKind = 'sample' | 'synth' | 'keys' | 'plugin';
type ChannelLike = { name: string; kind: ChannelKind; source?: number };
/** A source by id (a sample channel's sample), for icons read from what the sample is. */
export type SourceOf = (id: number) => { name: string; path: string } | undefined;

/**
 * A Channel Rack row's icon: a standard-library sample's group first (the library knows a
 * "Sleigh Bells" is percussion), then the channel's name, then its sample's name, then
 * 'piano', 'synth' (also a plugin instrument) or 'sample'.
 */
export function channelIcon(c: ChannelLike, src?: SourceOf): string {
  const s = c.kind === 'sample' && c.source != null ? src?.(c.source) : undefined;
  const i = libIcon(s?.path) ?? guessIcon(c.name) ?? (s ? guessIcon(s.name) : null);
  if (i) return i;
  if (c.kind === 'keys') return 'piano';
  return c.kind === 'synth' || c.kind === 'plugin' ? 'synth' : 'sample';
}

/**
 * A pattern's icon. A pattern that plays one channel (what "Split by channel" makes) shows that
 * channel's icon: its name repeats the parent pattern's ("Beat – 808"). Otherwise the name, else
 * the most used channel icon among its notes.
 */
export function patternIcon(p: { name: string; notes: readonly { ch: number }[] }, chans: ReadonlyMap<number, ChannelLike>, src?: SourceOf): string | null {
  const ch0 = p.notes[0]?.ch;
  if (ch0 !== undefined && p.notes.every(x => x.ch === ch0)) { const c = chans.get(ch0); if (c) return channelIcon(c, src); }
  const g = guessIcon(p.name); if (g) return g;
  const n = new Map<string, number>();
  for (const x of p.notes) { const c = chans.get(x.ch); if (c) { const id = channelIcon(c, src); n.set(id, (n.get(id) ?? 0) + 1); } }
  return mostUsed(n);
}

function mostUsed(n: ReadonlyMap<string, number>): string | null {
  let best: string | null = null, bn = 0;
  for (const [id, k] of n) if (k > bn) { best = id; bn = k; }
  return best;
}

/**
 * Per track, the icon shown when the track has none of its own and its name says nothing ("Track 7"):
 * the most common icon of the clips on it, so the lanes "Split by channel" fills are marked like
 * their clips. Null where the track has its own icon or holds no clip with one.
 */
export function laneAutoIcons(p: {
  tracks: readonly { icon?: string | null; name: string }[];
  clips: readonly { track: number; kind: string; ref: number }[];
  patterns: readonly { id: number; name: string; notes: readonly { ch: number }[] }[];
  channels?: readonly (ChannelLike & { id: number })[];
  sources: readonly { id: number; name: string; path: string }[];
}): (string | null)[] {
  const need = p.tracks.map(t => !trackIcon(t));
  if (!need.some(Boolean)) return p.tracks.map(() => null);
  const srcs = new Map(p.sources.map(s => [s.id, s])), src: SourceOf = id => srcs.get(id);
  const chans = new Map((p.channels ?? []).map(c => [c.id, c]));
  const pats = new Map(p.patterns.map(x => [x.id, x]));
  const patIcon = new Map<number, string | null>(), counts = p.tracks.map(() => new Map<string, number>());
  for (const c of p.clips) {
    if (!need[c.track]) continue;
    let id: string | null = null;
    if (c.kind === 'pat') {
      id = patIcon.get(c.ref) ?? null;
      if (!patIcon.has(c.ref)) { const pt = pats.get(c.ref); id = pt ? patternIcon(pt, chans, src) : null; patIcon.set(c.ref, id); }
    } else if (c.kind === 'audio') { const s = srcs.get(c.ref); id = s ? sampleIcon(s) : null; }
    if (id) counts[c.track].set(id, (counts[c.track].get(id) ?? 0) + 1);
  }
  return counts.map((n, i) => (need[i] ? mostUsed(n) : null));
}

const paths = new Map<string, { p: Path2D; s: number; w: number; h: number; eo: boolean }>();
/** Draw a glyph filled with the current fillStyle into a size×size box at (x, y). */
export function drawGlyph(g: CanvasRenderingContext2D, id: string, x: number, y: number, size: number) {
  const gl = INST_ICONS[id]; if (!gl) return;
  let c = paths.get(id);
  if (!c) { const [, , w, h] = gl.vb.split(' ').map(Number); c = { p: new Path2D(gl.d), s: 1 / Math.max(w, h), w, h, eo: !!gl.eo }; paths.set(id, c); }
  const k = size * c.s;
  g.save(); g.translate(x + (size - c.w * k) / 2, y + (size - c.h * k) / 2); g.scale(k, k); g.fill(c.p, c.eo ? 'evenodd' : 'nonzero'); g.restore();
}
