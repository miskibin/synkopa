// LV2 effects the in-browser mock "has installed", shaped like what the engine's lilv scan reads:
// control ports with real ranges, units, toggles, integers, log ranges and enumerations, plus one
// plugin that cannot run in Synkopa (and says why), so every branch of the plugin UI can be exercised.
import type { PortSpec } from '../lib/pluginParam';

/** `crashes`: it fails Synkopa's safety check (the engine refuses to load it until the user loads it anyway). */
export interface MockLv2 { name: string; vendor: string; category: string; bundle: string; unsupported?: string; crashes?: boolean; ports: PortSpec[] }

const DB = 'dB', HZ = 'Hz', MS = 'ms';

export const MOCK_LV2: Record<string, MockLv2> = {
  'http://gareus.org/oss/lv2/dpl#stereo': {
    name: 'x42-dpl Digital Peak Limiter Stereo', vendor: 'x42', category: 'Limiter', bundle: 'dpl.lv2',
    ports: [
      { id: 0, symbol: 'gain', name: 'Input Gain', min: -10, max: 30, def: 0, unit: DB },
      { id: 1, symbol: 'threshold', name: 'Threshold', min: -10, max: 0, def: 0, unit: DB },
      { id: 2, symbol: 'release', name: 'Release Time', min: 0.001, max: 1, def: 0.01, unit: 's', log: true },
      { id: 3, symbol: 'truepeak', name: 'True Peak', min: 0, max: 1, def: 0, toggle: true },
    ],
  },
  'http://plugin.org.uk/swh-plugins/amp': {
    name: 'Simple amplifier', vendor: 'Steve Harris', category: 'Amplifier', bundle: 'amp-swh.lv2',
    ports: [{ id: 0, symbol: 'gain', name: 'Amps gain', min: -70, max: 70, def: 0, unit: DB }],
  },
  'http://lsp-plug.in/plugins/lv2/para_equalizer_x8_stereo': {
    name: 'LSP Parametric Equalizer x8 Stereo', vendor: 'LSP LV2', category: 'Equaliser', bundle: 'lsp-plugins.lv2',
    ports: [
      { id: 4, symbol: 'mode', name: 'Equalizer mode', min: 0, max: 3, def: 0, integer: true, options: [[0, 'IIR'], [1, 'FIR'], [2, 'FFT'], [3, 'SPM']] },
      { id: 5, symbol: 'f_0', name: 'Frequency 0', min: 10, max: 24000, def: 16, unit: HZ, log: true },
      { id: 6, symbol: 'g_0', name: 'Gain 0', min: -36, max: 36, def: 0, unit: DB },
      { id: 7, symbol: 'q_0', name: 'Quality 0', min: 0, max: 100, def: 0 },
      { id: 8, symbol: 'ft_0', name: 'Filter type 0', min: 0, max: 2, def: 0, integer: true, options: [[0, 'Off'], [1, 'Bell'], [2, 'Hi-shelf']] },
      { id: 9, symbol: 's_0', name: 'Slope 0', min: 1, max: 4, def: 1, integer: true },
      { id: 10, symbol: 'att', name: 'Attack', min: 0, max: 250, def: 20, unit: MS },
    ],
  },
  'urn:example:lv2:fixed-block-gate': {
    name: 'Fixed-Block Gate', vendor: 'Example', category: 'Gate', bundle: 'fixed-gate.lv2',
    unsupported: 'it needs host features Synkopa does not provide: fixedBlockLength',
    ports: [],
  },
  'urn:example:lv2:crashy-vinyl': {
    name: 'Crashy Vinyl', vendor: 'Example', category: 'Simulator', bundle: 'crashy.lv2', crashes: true,
    ports: [{ id: 0, symbol: 'drive', name: 'Drive', min: 0, max: 1, def: 0.5 }],
  },
};
