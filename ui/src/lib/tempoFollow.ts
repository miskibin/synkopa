import { STRETCH_MIN, STRETCH_MAX, type Clip, type Project } from '../api/types';

/** Changes for a constant global BPM edit. Mirror engine ops.rs.
 * In both modes preserve source seconds = beats × 60 / BPM / stretch.
 * No mutations until all clips pass validation. A tempo map is a separate workflow.
 */
export function tempoClipChanges(p: Project, bpm: number): [number, Clip][] {
  if (!(bpm >= 20 && bpm <= 999)) throw new Error('bpm must be 20..999');
  if (p.tempoPoints?.length || bpm === p.bpm) return [];
  const ratio = bpm / p.bpm;
  return p.clips.flatMap((c, i): [number, Clip][] => {
    if (c.kind !== 'audio') return [];
    if (!c.followTempo) return [[i, { ...c, offset: c.offset * ratio, len: c.len * ratio }]];
    const stretch = (c.stretch ?? 1) / ratio;
    if (stretch < STRETCH_MIN - 1e-9 || stretch > STRETCH_MAX + 1e-9) throw new Error(`clip ${c.id}: tempo change exceeds stretch range`);
    const n = { ...c, stretch: Math.min(STRETCH_MAX, Math.max(STRETCH_MIN, stretch)) };
    if (n.stretch === 1) delete (n as Clip).stretch;
    return [[i, n]];
  });
}
