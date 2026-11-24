// The Synkopa Instruments instrument catalog as the engine has it (crates/engine/src/keys.rs CATALOG),
// read from the fixture the engine's test writes: the mock lists it and op validation checks
// instruments and articulations against it, so both stay in step with the engine.
import catalog from './__fixtures__/keys-catalog.json';
import type { KeysCategory } from '../api/types';

export interface KeysCatalogEntry {
  id: string; name: string; category: KeysCategory; description: string; kit: string; bundled: boolean;
  credit: string; license: string; range: [number, number]; articulations: { key: number; name: string }[];
  auto: { short: number; long: number; seconds: number } | null;
  stereo: boolean; releaseNoise: boolean;
  /** A fretted instrument's open strings (MIDI keys), empty for the rest: the engine plays each note on one. */
  strings: number[];
  /** Download size of the instrument's library kit. */
  bytes: number;
}

export const KEYS_CATALOG = catalog.instruments as KeysCatalogEntry[];

export function keysInstrument(id: string): KeysCatalogEntry | undefined {
  return KEYS_CATALOG.find(i => i.id === id);
}
