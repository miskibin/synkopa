// Hosted plugin parameters: how an LV2 control port's real value maps to the normalized 0..1
// that automation and `pluginSetParam` speak. The same rules as crates/lv2-host/src/params.rs
// (the mock uses them to behave like the engine; the generic editor uses `steps`).
import type { PluginParam } from '../api/types';

/** An LV2 control port's metadata (what the Turtle says). */
export interface PortSpec {
  id: number; symbol: string; name: string;
  min: number; max: number; def: number; unit?: string;
  integer?: boolean; toggle?: boolean; log?: boolean;
  /** Scale points of an enumeration: (value, label). */
  options?: [number, string][];
}

const usesLog = (s: PortSpec) => !!s.log && s.min > 0 && s.max > s.min && !s.toggle && !s.options?.length;
const points = (s: PortSpec) => [...(s.options ?? [])].sort((a, b) => a[0] - b[0]);

/** Snap a real value to what the port accepts. */
export function snap(s: PortSpec, v: number): number {
  if (!Number.isFinite(v)) return s.def;
  if (s.toggle) return v > (s.min + s.max) / 2 ? s.max : s.min;
  const pts = points(s);
  if (pts.length) return pts.reduce((best, p) => (Math.abs(p[0] - v) < Math.abs(best - v) ? p[0] : best), pts[0][0]);
  const c = Math.min(s.max, Math.max(s.min, v));
  return s.integer ? Math.round(c) : c;
}

export function fromNormalized(s: PortSpec, n: number): number {
  if (!Number.isFinite(n)) return s.def;
  n = Math.min(1, Math.max(0, n));
  const pts = points(s);
  if (pts.length) return pts[Math.min(pts.length - 1, Math.floor(n * pts.length))][0];
  return snap(s, usesLog(s) ? s.min * Math.pow(s.max / s.min, n) : s.min + n * (s.max - s.min));
}

export function toNormalized(s: PortSpec, v: number): number {
  v = snap(s, v);
  const pts = points(s);
  if (pts.length) return (Math.max(0, pts.findIndex(p => p[0] === v)) + 0.5) / pts.length;
  if (s.max <= s.min) return 0;
  const n = usesLog(s) ? Math.log(v / s.min) / Math.log(s.max / s.min) : (v - s.min) / (s.max - s.min);
  return Math.min(1, Math.max(0, n));
}

export function display(s: PortSpec, v: number): string {
  if (s.toggle) return v > (s.min + s.max) / 2 ? 'On' : 'Off';
  const pt = s.options?.find(p => p[0] === v);
  if (pt) return pt[1];
  const a = Math.abs(v);
  const num = s.integer ? v.toFixed(0) : a >= 100 || a === 0 ? v.toFixed(0) : a >= 10 ? v.toFixed(1) : a >= 1 ? v.toFixed(2) : v.toFixed(3);
  return !s.unit ? num : s.unit === '%' ? num + '%' : num + ' ' + s.unit;
}

/** Discrete positions, or undefined for a continuous control. */
export function steps(s: PortSpec): number | undefined {
  if (s.toggle) return 2;
  if (s.options?.length) return s.options.length;
  if (s.integer) { const n = Math.floor(s.max) - Math.ceil(s.min) + 1; return n >= 1 && n <= 128 ? n : undefined; }
  return undefined;
}

/** The parameter as `pluginParams` reports it for an LV2 port holding `real`. */
export function lv2Param(s: PortSpec, real: number): PluginParam {
  const st = steps(s);
  return {
    id: s.id, symbol: s.symbol, name: s.name, value: toNormalized(s, real), display: display(s, real), def: toNormalized(s, s.def),
    real, min: s.min, max: s.max, unit: s.unit ?? '', toggle: !!s.toggle, integer: !!s.integer, log: usesLog(s),
    ...(st != null ? { steps: st } : {}),
    ...(s.options?.length ? { options: points(s).map(([v, label]) => ({ value: toNormalized(s, v), label })) } : {}),
  };
}

/** The slider step of a parameter in normalized units (a discrete control moves a whole position). */
export function sliderStep(p: Pick<PluginParam, 'steps'>): number {
  return p.steps && p.steps > 1 ? 1 / (p.steps - 1) : 0.001;
}
