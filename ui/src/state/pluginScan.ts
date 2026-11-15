// The installed plugins, scanned once per session and shared by the FX picker (effects) and the
// Channel Rack's instrument menus (VST3 instruments). A rescan opens only modules it has not seen.
import { createSignal } from 'solid-js';
import { be } from '../api/backend';
import type { PluginScan } from '../api/types';
import { say } from './ui';

const [scan, setScan] = createSignal<PluginScan | null>(null);
let scanning: Promise<PluginScan> | null = null;

/** The last scan, or null while none has finished. */
export const pluginScan = scan;

/** Scans (`rescan`: also opens new or changed modules); resolves with the result. One at a time. */
export function runPluginScan(rescan: boolean): Promise<PluginScan> {
  if (scanning) return scanning;
  if (rescan) setScan(null);
  scanning = be().pluginScan(rescan)
    .catch(e => { say(String(e)); return { plugins: [], instruments: [], unavailable: {} } as PluginScan; })
    .then(r => { setScan(r); return r; })
    .finally(() => { scanning = null; });
  return scanning;
}

/** Starts the cached scan (opens nothing) if none has run yet. */
export const ensurePluginScan = () => { if (!scan()) void runPluginScan(false); };

/** Shares a scan made elsewhere (Settings' rescan) with every list. */
export const setPluginScan = (s: PluginScan) => setScan(s);
