// "Move notes to new pattern": the selected notes leave their pattern for a new one, at the
// same steps, and the arrangement keeps sounding the same. FL has no single command for this
// (cut, new pattern, paste lands elsewhere; a playlist slice made unique keeps every note), and
// doing it by hand silently drops the part from every place the pattern is used.
import type { Clip, Id, Note, Op, Pattern, Project } from '../api/types';
import { barOf, minLenOfIn } from './pattern';
import { newTrack } from './clipOps';

const EPS = 1e-6;

export interface MovePlan {
  ops: Op[];
  pattern: Pattern;
  /** Clips of the new pattern placed in the playlist (one beside each clip of the source). */
  placed: number;
  /** Tracks added because no lane below a clip was free there. */
  addedTracks: number;
}

/**
 * The ops that move notes `idx` (indices into `src.notes`) to a new pattern named `name`.
 * Both patterns keep the source's length (`minLen`), so their clips stay in time and loop alike.
 * Each playlist clip of the source gets a clip of the new pattern with the same start, length
 * and offset on the nearest lane below that is free over that span (a new lane when none is):
 * never stacked on another clip.
 */
export function planMoveNotes(p: Project, src: Pattern, idx: readonly number[], name: string, color: string, newId: () => Id): MovePlan | null {
  const take = new Set(idx.filter(i => i >= 0 && i < src.notes.length));
  if (!take.size) return null;
  const moved: Note[] = [], kept: Note[] = [];
  src.notes.forEach((n, i) => (take.has(i) ? moved : kept).push({ ...n }));
  // whole bars of the source's own grid (a 7/8 pattern keeps 3.5-beat bars)
  const bar = barOf(src);
  const minLen = Math.max(minLenOfIn(src, bar), Math.ceil(src.len / bar - EPS) * bar);
  // the moved notes keep their channel's loop, so what they sound like does not change
  // (single-channel moves only: mixed notes carry no one channel's loop)
  const movedChs = new Set(moved.map(n => n.ch));
  // every moved channel keeps its loop, so what the notes sound like does not change
  const loops: Record<string, number> = {};
  for (const id of movedChs) { const l = src.channelLoops?.[String(id)]; if (l != null && l > 0) loops[String(id)] = l; }
  const pattern: Pattern = { id: newId(), name, color, len: src.len, minLen, notes: moved, ...(src.bar != null ? { bar: src.bar } : {}), ...(src.swing != null ? { swing: src.swing } : {}), ...(Object.keys(loops).length ? { channelLoops: loops } : {}) };
  const ops: Op[] = [
    { op: 'putPattern', pattern: { ...src, minLen, notes: kept } },
    { op: 'putPattern', pattern },
  ];
  // ---- playlist: a clip of the new pattern beside every clip of the source
  const lanes: Clip[][] = p.tracks.map(() => []);
  for (const c of p.clips) lanes[c.track]?.push(c);
  const free = (t: number, a: number, b: number) => !(lanes[t] ?? []).some(c => c.start < b - EPS && c.start + c.len > a + EPS);
  const clips: Clip[] = [];
  let nTracks = p.tracks.length, addedTracks = 0;
  const sources = p.clips.filter(c => c.kind === 'pat' && c.ref === src.id).sort((x, y) => x.start - y.start || x.track - y.track);
  for (const c of sources) {
    const a = c.start, b = c.start + c.len;
    let t = c.track + 1;
    while (t < nTracks && !free(t, a, b)) t++;
    if (t >= nTracks) {
      ops.push({ op: 'putTrack', track: newTrack(newId(), nTracks) });
      lanes.push([]); t = nTracks++; addedTracks++;
    }
    const nc: Clip = { ...c, id: newId(), track: t, ref: pattern.id };
    lanes[t].push(nc); clips.push(nc);
  }
  if (clips.length) ops.push({ op: 'putClips', clips });
  return { ops, pattern, placed: clips.length, addedTracks };
}
