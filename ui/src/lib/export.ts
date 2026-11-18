import { VOL_UNITY, type Id, type Insert, type LoopRange, type Project } from '../api/types';

/** Beat range for a song, loop or pattern bounce — mirrors `Engine::export_range` / `Engine::pattern_range`. */
export function exportRange(project: Project, useLoop: boolean, pattern?: Id | null): { from: number; to: number } {
  if (pattern != null) {
    const pt = project.patterns.find(x => x.id === pattern);
    return { from: 0, to: Math.max(4, pt?.len ?? 4) };
  }
  if (useLoop) {
    const l = project.loop;
    if (l && l.b > l.a) return { from: l.a, to: l.b };
  }
  return { from: 0, to: songEnd(project) };
}

/** How far (beats) a clip may run past a bar line before the song gets that extra bar (model.rs `SONG_END_SLACK`). */
export const SONG_END_SLACK = 0.25;
/**
 * The bar the last clip ends in; at least one bar (model.rs `song_end`). A clip running into the
 * next bar by less than a 16th does not add it; a clip that starts in a bar always keeps it.
 */
export function songEnd(project: Pick<Project, 'clips'>): number {
  let end = 4;
  for (const c of project.clips) end = Math.max(end, c.start + c.len - SONG_END_SLACK, c.start + 1e-6);
  return Math.ceil(end / 4) * 4;
}

/**
 * The project as a pattern export hears it, like PAT mode plays it — mirrors engine.rs `pattern_project`:
 * pattern `id` once from beat 0 on the first track (unmuted, unity, into the master) and nothing else
 * of the arrangement. Its channels play into their own inserts, or the master.
 */
export function patternProject(p: Project, id: Id): Project {
  const q = structuredClone(p);
  const len = Math.max(4, q.patterns.find(x => x.id === id)?.len ?? 4);
  q.clips = []; if (q.comps) q.comps = []; q.loop = null;
  if (!q.tracks.length) q.tracks.push({ id: 1, name: 'Pattern', color: '#4f8ef7', muted: false, solo: false, arm: false, vol: VOL_UNITY, h: 56, insert: 0 });
  q.tracks.forEach((t, i) => { t.solo = false; if (i === 0) { t.muted = false; t.vol = VOL_UNITY; t.insert = 0; } });
  q.clips.push({ id: q.nextId++, track: 0, start: 0, len, offset: 0, kind: 'pat', ref: id, muted: false, gain: 0, fadeIn: 0, fadeOut: 0, pan: 0, norm: false, rev: false });
  return q;
}

/** A pattern export would be silent: the pattern has no note on an unmuted channel. */
export function patternSilent(p: Project, id: Id): boolean {
  const pt = p.patterns.find(x => x.id === id);
  const chans = p.channels ?? [];
  return !pt || !pt.notes.some(n => chans.some(c => c.id === n.ch && !c.mute));
}

/** Wall-clock length of a bounce including the effect tail. */
export function exportSeconds(from: number, to: number, bpm: number, tail: number): number {
  const beats = Math.max(0, to - from);
  return (beats * 60) / Math.max(1, bpm) + Math.max(0, tail);
}

export function fmtExportLen(seconds: number): string {
  if (seconds < 60) return seconds.toFixed(1).replace(/\.0$/, '') + ' s';
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds - m * 60);
  return m + ':' + String(s).padStart(2, '0');
}

export function hasLoop(loop: LoopRange | null | undefined): boolean {
  return !!loop && loop.b > loop.a;
}

/**
 * The inserts a stem export writes a file for, in mixer order — mirrors `Engine::stem_inserts`: not the
 * master, not muted, with a track that has an unmuted clip or an unmuted channel sent to it. (The engine
 * also drops one that renders silent, so the real count can be lower.)
 */
export function stemInserts(project: Project): Insert[] {
  return project.inserts.filter(m => m.id !== 0 && !m.muted).filter(m => {
    const onTrack = project.tracks.some((t, i) => t.insert === m.id && !t.muted && project.clips.some(c => c.track === i && !c.muted));
    const onChannel = (project.channels ?? []).some(c => c.insert === m.id && !c.mute);
    return onTrack || onChannel;
  });
}

/** A file name safe on Windows, macOS and Linux — mirrors `actions::safe_file_stem`. */
export function safeFileStem(name: string): string {
  // eslint-disable-next-line no-control-regex
  let s = name.replace(/[\u0000-\u001f\u007f<>:"/\\|?*]/g, '_').trim().replace(/[. ]+$/, '');
  s = Array.from(s).slice(0, 100).join('').replace(/ +$/, '');
  if (!s) return 'Insert';
  const base = s.split('.')[0].toUpperCase();
  return /^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])$/.test(base) ? '_' + s : s;
}

/** File stems for `names` in one folder: made safe and unique with " (2)", " (3)"; case-insensitive — mirrors `actions::unique_file_stems`. */
export function uniqueFileStems(names: string[]): string[] {
  const taken = new Set<string>();
  return names.map(n => {
    const base = safeFileStem(n);
    let cand = base;
    for (let k = 2; taken.has(cand.toLowerCase()); k++) cand = `${base} (${k})`;
    taken.add(cand.toLowerCase());
    return cand;
  });
}

/** What a file of the chosen format is: WAV at a bit depth (32 = float) or MP3 at a bitrate. */
export type ExportEncoding = { format: 'wav'; bits: 16 | 24 | 32 } | { format: 'mp3'; kbps: number };

/** Approximate size of one stereo file `seconds` long: WAV is exact up to its header; MP3 is CBR. */
export function exportBytes(seconds: number, rate: number, enc: ExportEncoding): number {
  const s = Math.max(0, seconds);
  if (enc.format === 'mp3') return Math.round(s * enc.kbps * 1000 / 8);
  return 44 + Math.round(s * rate) * 2 * (enc.bits / 8);
}

/** The bit depth as the dialog and the summary name it. */
export function depthText(bits: 16 | 24 | 32): string {
  return bits === 32 ? '32-bit float' : bits + '-bit PCM';
}

/** Share of one file's time each stage takes, for the overall bar (rendering dominates). */
const PREPARE = 0.05, WRITE = 0.95;

/** 0‥1 through the whole export: finished files plus how far the current one is. */
export function exportOverall(p: { done: number; total: number; stage: string; frac: number }): number {
  const total = Math.max(1, p.total);
  if (p.stage === 'done' || p.done >= total) return 1;
  const f = Math.min(1, Math.max(0, p.frac));
  const inFile = p.stage === 'prepare' ? PREPARE * f : p.stage === 'render' ? PREPARE + (WRITE - PREPARE) * f : WRITE;
  return Math.min(1, (p.done + inFile) / total);
}

/** Milliseconds left, extrapolated from the time so far; null until there is enough to go on. */
export function exportRemaining(elapsedMs: number, overall: number): number | null {
  if (elapsedMs < 600 || overall < 0.03) return null;
  if (overall >= 1) return 0;
  return elapsedMs * (1 - overall) / overall;
}

/** m:ss for a duration in milliseconds (rounded up, so "0:00" means done). */
export function fmtMs(ms: number): string {
  const s = Math.max(0, Math.ceil(ms / 1000));
  return Math.floor(s / 60) + ':' + String(s % 60).padStart(2, '0');
}

/** SVG path of a peak waveform (0‥255 per slice), mirrored around the middle of a `w`×`h` box. */
export function wavePath(wave: readonly number[], w: number, h: number): string {
  if (!wave.length) return '';
  const dx = w / wave.length, mid = h / 2;
  let d = '';
  wave.forEach((v, i) => {
    const a = Math.max(0.5, (Math.min(255, v) / 255) * mid);
    if (v > 0) d += `M${(i * dx).toFixed(2)} ${(mid - a).toFixed(2)}h${Math.max(0.5, dx * 0.8).toFixed(2)}v${(2 * a).toFixed(2)}h${(-Math.max(0.5, dx * 0.8)).toFixed(2)}z`;
  });
  return d;
}
