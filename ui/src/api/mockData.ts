// Data for the in-browser mock backend: the catalog of crates/engine/src/catalog.rs and the
// demo song of crates/engine/src/demo.rs. Only the mock uses these; the real UI reads `fx_catalog`.
import { VOL_UNITY, type AnyBuiltinKind, type Catalog, type Clip, type EqBand, type Note, type ParamDef, type Preset, type Project, type WarpMaps } from './types';
import { defaultBands, loadPreset, newFx } from '../lib/presets';
import { addStarterKit, factoryChannel } from '../lib/factory';
export { demoProject, emptyProject } from '../lib/factory';
import { SYNTH_CATALOG } from './synthCatalog';
import warpFactoryJson from '../../../crates/engine/src/warp_factory.json?raw';
import warpLibraryJson from '../../../crates/engine/src/warp_library.json?raw';

/** catalog.rs `warp`: the factory slots, from the same file the engine embeds. */
function warpFactory(): WarpMaps {
  const f = JSON.parse(warpFactoryJson) as WarpMaps;
  return { time: f.time, vol: f.vol };
}

const p = (id: string, label: string, min: number, max: number, def: number, unit: string, log = false): ParamDef => ({ id, label, min, max, def, unit, log });
/** catalog.rs `choice`: a discrete parameter (value = index) shown as buttons. */
const choice = (id: string, label: string, def: number, options: string[]): ParamDef => ({ id, label, min: 0, max: options.length - 1, def, unit: '', log: false, options });

const SLOTS = Array.from({ length: 16 }, (_, i) => String(i + 1));
const PARAMS: Record<AnyBuiltinKind, ParamDef[]> = {
  eq: [p('out', 'Output', -18, 18, 0, 'dB')],
  comp: [p('thr', 'Thresh', -60, 0, -18, 'dB'), p('ratio', 'Ratio', 1, 20, 4, ':1', true), p('att', 'Attack', 0.1, 100, 10, 'ms', true),
    p('rel', 'Release', 10, 1000, 150, 'ms', true), p('gain', 'Makeup', 0, 24, 0, 'dB'), choice('det', 'Detect', 0, ['Peak', 'RMS'])],
  // catalog.rs SAT (dsp::saturator::idx order)
  sat: [choice('style', 'Style', 0, ['Taśma', 'Konsola', 'Transformator', 'Trioda', 'Pentoda']), p('drive', 'Drive', 0, 100, 40, '%'),
    choice('punish', 'Punish', 0, ['Off', 'On']), p('lowcut', 'Low cut', 10, 1000, 10, 'Hz', true), p('thump', 'Thump', 0, 100, 0, '%'),
    p('tone', 'Tone', -100, 100, 0, '%'), p('highcut', 'High cut', 500, 20000, 20000, 'Hz', true), choice('steep', 'Steep', 0, ['Off', 'On']),
    choice('auto', 'Auto gain', 1, ['Off', 'On']), p('mix', 'Mix', 0, 100, 100, '%'), p('out', 'Output', -24, 12, 0, 'dB'),
    choice('hq', 'Oversampling', 0, ['2×', '4×'])],
  // catalog.rs DELAY (dsp::delay::idx order)
  delay: [p('input', 'Input', 0, 100, 100, '%'), choice('sync', 'Sync', 0, ['Tempo', 'ms']), p('beats', 'Time', 1 / 48, 4, 0.75, 'beat', true),
    p('time', 'Time', 1, 2000, 375, 'ms', true), choice('keep', 'Keep pitch', 0, ['Off', 'On']), p('smooth', 'Smoothing', 0, 100, 40, '%'),
    p('offset', 'Offset', -100, 100, 0, '%'), p('pan', 'Pan', -100, 100, -60, '%'), choice('model', 'Model', 1, ['Mono', 'Stereo', 'Ping-pong', 'Off']),
    p('spread', 'Stereo', 0, 100, 100, '%'), p('fb', 'Feedback', 0, 125, 35, '%'), choice('ftype', 'Filter', 1, ['Off', 'LP', 'HP', 'BP']),
    p('cutoff', 'Cutoff', 20, 20000, 6000, 'Hz', true), p('res', 'Resonance', 0, 100, 0, '%'), p('srate', 'Sample rate', 500, 48000, 48000, 'Hz', true),
    p('bits', 'Bits', 2, 24, 24, 'bit'), choice('dist', 'Distortion', 1, ['Limit', 'Sat']), p('level', 'Threshold', -36, 0, 0, 'dB'),
    p('knee', 'Knee', 0, 100, 50, '%'), p('sym', 'Symmetry', 0, 100, 0, '%'), p('diff', 'Diffusion', 0, 100, 0, '%'),
    p('dspread', 'Diff. spread', 0, 100, 50, '%'), p('rate', 'LFO rate', 0.05, 20, 0.5, 'Hz', true), p('mtime', 'Mod time', 0, 100, 0, '%'),
    p('mcut', 'Mod cutoff', 0, 100, 0, '%'), p('mix', 'Wet', 0, 100, 25, '%'), p('tone', 'Tone', -100, 100, 0, '%'), p('dry', 'Dry', 0, 100, 100, '%'),
    p('duck', 'Ducking', 0, 12, 0, 'dB')],
  // catalog.rs REVERB / LIMIT (v2)
  reverb: [p('size', 'Decay', 0.2, 20, 1, 's', true), p('pre', 'Pre-delay', 0, 150, 25, 'ms'), p('lo', 'Low cut', 20, 1000, 300, 'Hz', true),
    p('hi', 'High cut', 2000, 16000, 7000, 'Hz', true), p('width', 'Width', 0, 100, 100, '%'), p('mix', 'Mix', 0, 100, 15, '%'),
    p('tone', 'Tone', -100, 100, 0, '%'), choice('space', 'Space', 0, ['Plate', 'Room', 'Hall', 'Ambience']), p('duck', 'Ducking', 0, 12, 0, 'dB'),
    p('early', 'Early', 0, 100, 50, '%'), p('bass', 'Bass decay', 0.5, 1.5, 1, '×'), p('mod', 'Modulation', 0, 100, 50, '%')],
  limit: [p('gain', 'Gain', 0, 24, 0, 'dB'), p('ceil', 'Ceiling', -12, 0, -1, 'dBTP'), p('rel', 'Release', 10, 1000, 80, 'ms', true),
    choice('auto', 'Auto release', 1, ['Off', 'On']), p('look', 'Lookahead', 1, 5, 3, 'ms'), p('link', 'Stereo link', 0, 100, 100, '%'), p('out', 'Output', -12, 0, 0, 'dB')],
  fet: [p('input', 'Input', 0, 36, 8, 'dB'), p('output', 'Output', -24, 12, 0, 'dB'), choice('ratio', 'Ratio', 0, ['4', '8', '12', '20', 'All']),
    p('attack', 'Attack', 20, 800, 300, 'µs', true), p('release', 'Release', 50, 1100, 300, 'ms', true), p('mix', 'Mix', 0, 100, 100, '%')],
  deess: [p('thr', 'Threshold', -60, 0, -32, 'dB'), p('range', 'Range', 0, 24, 8, 'dB'), p('freq', 'Freq', 2000, 16000, 6000, 'Hz', true),
    choice('mode', 'Mode', 0, ['Split', 'Wide']), p('look', 'Lookahead', 0, 15, 5, 'ms'), choice('listen', 'Listen', 0, ['Off', 'S']),
    choice('detect', 'Detect', 0, ['Absolute', 'Relative'])],
  denoise: [p('reduction', 'Reduction', 0, 40, 12, 'dB'), p('sens', 'Sensitivity', 0, 100, 50, '%'), p('smooth', 'Smoothing', 0, 100, 50, '%'),
    choice('learn', 'Profile', 0, ['Adaptive', 'Hold']), choice('listen', 'Listen', 0, ['Off', 'Noise'])],
  tune: [choice('key', 'Key', 0, ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B']),
    choice('scale', 'Scale', 0, ['Chromatic', 'Major', 'Minor', 'Maj pent', 'Min pent']),
    p('retune', 'Retune', 0, 400, 20, 'ms'), p('humanize', 'Humanize', 0, 100, 0, '%'), p('flex', 'Flex', 0, 100, 0, '%'),
    p('vibrato', 'Vibrato', 0, 100, 0, '%'), p('transpose', 'Transpose', -12, 12, 0, 'st'), p('formant', 'Formant', -12, 12, 0, 'st'),
    choice('voice', 'Voice', 0, ['Auto', 'Low male', 'Male', 'Female', 'Soprano'])],
  voice: [choice('mode', 'Mode', 0, ['Transpose', 'Quantize', 'Robot']), p('pitch', 'Pitch', -12, 12, 0, 'st'), p('formant', 'Formant', -12, 12, 0, 'st'),
    choice('link', 'Link', 0, ['Off', 'On']), p('drive', 'Drive', 0, 30, 0, 'dB'), p('out', 'Output', -24, 12, 0, 'dB')],
  // catalog.rs RIDER (order = rider::idx)
  rider: [p('amount', 'Amount', 0, 100, 50, '%'), p('phrase', 'Phrase', 0, 100, 20, '%'), p('up', 'Max boost', 0, 12, 5, 'dB'),
    p('down', 'Max cut', 0, 12, 6, 'dB'), choice('speed', 'Speed', 0, ['Sung', 'Rap']), choice('auto', 'Auto target', 1, ['Off', 'On']),
    p('target', 'Target', -60, 0, -24, 'dB'), p('floor', 'Noise floor', -100, -20, -70, 'dB'), choice('look', 'Look-ahead', 2, ['Krótki (+0,3 s opóźnienia)', 'Długi (+1–1,6 s opóźnienia)', 'Na żywo']),
    p('out', 'Output', -24, 24, 0, 'dB'), p('gate', 'Gate', -100, -10, -60, 'dB')],
  // catalog.rs HALF (order = dsp::halftime::idx)
  // catalog.rs WARP (dsp::warp::idx order)
  warp: [choice('tslot', 'Time slot', 0, SLOTS), choice('vslot', 'Volume slot', 0, SLOTS), choice('trig', 'Trigger sync', 0, ['Off', '1/16', '1/8', '1/4', '1/2', '1 takt']),
    choice('pos', 'Position', 0, ['Song', 'Trigger']), choice('oneshot', 'One-shot', 0, ['Off', 'On']), choice('hold', 'Hold', 0, ['Off', 'Poprzedni takt', 'Następny takt']),
    p('tmix', 'Time mix', 0, 100, 100, '%'), p('vmix', 'Volume mix', 0, 100, 100, '%'), p('att', 'Attack', 0, 500, 0, 'ms'), p('rel', 'Release', 0, 1000, 0, 'ms'),
    p('tns', 'Tension', -100, 100, 0, '%'), choice('click', 'Click reduction', 1, ['Off', 'On']), choice('attack', 'Attack protect', 1, ['Off', 'On']), choice('dc', 'DC removal', 1, ['Off', 'On'])],
  // catalog.rs CHORUS / PHASER (dsp::modulation::chorus_idx / phaser_idx order)
  chorus: [choice('mode', 'Mode', 0, ['Chorus', 'Ensemble', 'Vibrato']), choice('sync', 'Rate', 0, ['Hz', 'Tempo']), p('rate', 'Rate', 0.05, 10, 0.6, 'Hz', true),
    choice('div', 'Rate', 4, ['1/16', '1/8', '1/4', '1/2', '1 takt', '2 takty', '4 takty', '8 taktów']), p('depth', 'Depth', 0, 100, 40, '%'), p('delay', 'Delay', 2, 40, 12, 'ms', true),
    p('fb', 'Feedback', -90, 90, 0, '%'), p('width', 'Width', 0, 200, 100, '%'), p('hp', 'Safe bass', 20, 1000, 20, 'Hz', true),
    p('tone', 'Tone', 1000, 20000, 20000, 'Hz', true), p('mix', 'Mix', 0, 100, 50, '%')],
  phaser: [choice('stages', 'Stages', 1, ['2', '4', '6', '8', '12']), choice('sync', 'Rate', 0, ['Hz', 'Tempo']), p('rate', 'Rate', 0.02, 10, 0.4, 'Hz', true),
    choice('div', 'Rate', 4, ['1/16', '1/8', '1/4', '1/2', '1 takt', '2 takty', '4 takty', '8 taktów']), p('center', 'Center', 100, 8000, 800, 'Hz', true), p('depth', 'Depth', 0, 100, 70, '%'),
    p('fb', 'Feedback', -95, 95, 30, '%'), p('stereo', 'Stereo', 0, 180, 90, '°'), p('mix', 'Mix', 0, 100, 50, '%')],
  // catalog.rs STEREO (dsp::stereo::idx order)
  stereo: [p('width', 'Width', 0, 200, 100, '%'), p('spread', 'Spread', 0, 100, 0, '%'), choice('bass', 'Bass mono', 0, ['Off', 'On']),
    p('bassf', 'Mono below', 40, 500, 120, 'Hz', true), p('gain', 'Gain', -60, 24, 0, 'dB'), p('pan', 'Pan', -100, 100, 0, '%'),
    choice('invl', 'Invert L', 0, ['Off', 'On']), choice('invr', 'Invert R', 0, ['Off', 'On']), choice('swap', 'Swap L/R', 0, ['Off', 'On'])],
  // catalog.rs ANALYZER
  analyzer: [choice('target', 'Target', 1, ['Off', 'Streaming −14', 'Loud −9', 'Broadcast −23']), choice('slope', 'Slope', 2, ['0 dB', '3 dB', '4.5 dB'])],
  // catalog.rs MULTIBAND (dsp::multiband::idx order)
  multiband: [p('depth', 'Depth', 0, 100, 40, '%'), p('time', 'Time', 10, 1000, 100, '%', true), p('up', 'Upward', 0, 200, 100, '%'),
    p('down', 'Downward', 0, 200, 100, '%'), p('in', 'In gain', -24, 24, 0, 'dB'), p('out', 'Out gain', -24, 24, 0, 'dB'),
    p('xlo', 'Low / mid', 30, 1000, 88.3, 'Hz', true), p('xhi', 'Mid / high', 500, 16000, 2500, 'Hz', true),
    p('gl', 'Low gain', -24, 24, 0, 'dB'), p('gm', 'Mid gain', -24, 24, 0, 'dB'), p('gh', 'High gain', -24, 24, 0, 'dB'),
    p('dl', 'Low down thresh', -72, 0, -33.8, 'dB'), p('dm', 'Mid down thresh', -72, 0, -30.3, 'dB'), p('dh', 'High down thresh', -72, 0, -35.5, 'dB'),
    p('ul', 'Low up thresh', -90, 0, -40.8, 'dB'), p('um', 'Mid up thresh', -90, 0, -41.8, 'dB'), p('uh', 'High up thresh', -90, 0, -40.8, 'dB')],
  half: [choice('speed', 'Speed', 1, ['1.5×', '2×', '4×']), choice('loop', 'Loop', 4, ['1/16', '1/8', '1/4', '1/2', '1 takt', '2 takty', '4 takty', '8 taktów']),
    p('smooth', 'Smooth', -100, 100, 10, '%'), p('lo', 'Band low', 20, 20000, 20, 'Hz', true), p('hi', 'Band high', 20, 20000, 20000, 'Hz', true),
    choice('solo', 'Band solo', 0, ['Off', 'On']), p('mix', 'Mix', 0, 100, 100, '%')],
};
const NAMES: Record<AnyBuiltinKind, string> = { eq: 'Parametric EQ', comp: 'Compressor', sat: 'Saturator', delay: 'Delay', reverb: 'Reverb', limit: 'Limiter',
  fet: 'FET Compressor', deess: 'De-esser', tune: 'Pitch Correct', denoise: 'Denoiser', voice: 'Voice', rider: 'Vocal Rider', half: 'Half-Speed', warp: 'Warp', chorus: 'Chorus', phaser: 'Phaser',
  stereo: 'Stereo', analyzer: 'Analyzer', multiband: 'Multiband' };

type BandEdit = [number, boolean | null, number | null, number | null, number | null];
function eqp(name: string, out: number, edits: BandEdit[]): Preset {
  const bands: EqBand[] = defaultBands();
  for (const [i, on, f, g, q] of edits) {
    const b = bands[i];
    if (on != null) b.on = on; if (f != null) b.f = f; if (g != null) b.g = g; if (q != null) b.q = q;
  }
  return { name, params: { out }, bands };
}
/** catalog.rs `eqo`: only the listed bands stay on; cut bands get `slope`. */
function eqo(name: string, out: number, slope: number, edits: BandEdit[]): Preset {
  const p = eqp(name, out, edits);
  p.bands!.forEach((b, i) => {
    if (!edits.some(e => e[0] === i)) b.on = false;
    else if (b.type === 'lowcut' || b.type === 'highcut') b.slope = slope;
  });
  return p;
}
const pr = (name: string, params: Record<string, number>): Preset => ({ name, params });
/** catalog.rs `dly` / `dly_ms`: synced (beats) or free (ms) delay presets. */
const dly = (name: string, beats: number, fb: number, mix: number, kv: Record<string, number> = {}): Preset => ({ name, params: { ...kv, sync: 0, beats, fb, mix } });
const dlyMs = (name: string, time: number, fb: number, mix: number, kv: Record<string, number> = {}): Preset => ({ name, params: { ...kv, sync: 1, time, fb, mix } });
const T = true, n = null;

/** catalog.rs chorus presets: (name, mode, rate, depth, delay, feedback, width, safe bass, tone, mix). */
const cho = (name: string, mode: number, rate: number, depth: number, delay: number, fb: number, width: number, hp: number, tone: number, mix: number, kv: Record<string, number> = {}): Preset =>
  ({ name, params: { mode, rate, depth, delay, fb, width, hp, tone, mix, ...kv } });
/** catalog.rs phaser presets: (name, stages index, rate, center, depth, feedback, stereo, mix). */
const pha = (name: string, stages: number, rate: number, center: number, depth: number, fb: number, stereo: number, mix: number, kv: Record<string, number> = {}): Preset =>
  ({ name, params: { stages, rate, center, depth, fb, stereo, mix, ...kv } });

/** catalog.rs stereo presets: (name, width, spread, bass mono, mono below). */
const ste = (name: string, width: number, spread: number, bass: number, bassf: number): Preset => ({ name, params: { width, spread, bass, bassf } });
/** catalog.rs multiband presets: (name, depth, time, up, down, in, out, low/mid, mid/high, band gains). */
const mbp = (name: string, depth: number, time: number, up: number, down: number, inp: number, out: number, xlo: number, xhi: number, g: [number, number, number]): Preset =>
  ({ name, params: { depth, time, up, down, in: inp, out, xlo, xhi, gl: g[0], gm: g[1], gh: g[2] } });

const PRESETS: Record<AnyBuiltinKind, Preset[]> = {
  stereo: [
    ste('Bez zmian', 100, 0, 0, 120),
    ste('Bas w mono (120 Hz)', 100, 0, 1, 120),
    ste('Szerzej, bas w mono', 140, 0, 1, 150),
    ste('Mono → stereo (spread)', 100, 60, 1, 150),
    ste('Wokal — dubel szeroki', 120, 35, 1, 200),
    ste('Pad — bardzo szeroko', 170, 40, 1, 200),
    ste('Węziej (70 %)', 70, 0, 0, 120),
    ste('Mono', 0, 0, 0, 120),
  ],
  analyzer: [pr('Streaming (−14 LUFS)', { target: 1 }), pr('Głośno (−9 LUFS)', { target: 2 }), pr('Broadcast (−23 LUFS)', { target: 3 })],
  multiband: [
    mbp('Domyślny — klej', 40, 100, 100, 100, 0, 0, 88.3, 2500, [0, 0, 0]),
    mbp('Klasyczny OTT (100 %)', 100, 100, 100, 100, 0, 0, 88.3, 2500, [0, 0, 0]),
    mbp('Hi-haty — chrupko', 45, 40, 110, 90, 0, 0, 120, 3200, [-6, -1, 2]),
    mbp('Lead — gęsto', 65, 80, 125, 110, 0, -1.5, 100, 2400, [-3, 1, 1.5]),
    mbp('808 — trzyma dół', 40, 160, 40, 120, 0, 0, 90, 1200, [0, -1, -6]),
    mbp('Wokal — bliżej', 25, 90, 90, 85, 0, 0, 110, 3000, [-6, 0, 1.5]),
    mbp('Pad — powietrze', 50, 150, 140, 60, 0, -1, 150, 3500, [-3, -1, 2]),
    mbp('Master — lekko', 12, 150, 55, 75, 0, 0, 95, 2800, [0, 0, 0]),
  ],
  chorus: [
    cho('Klasyczny chorus', 0, 0.6, 40, 12, 0, 100, 20, 20000, 50),
    cho('Wokal — zagęszczenie', 0, 0.35, 18, 16, 0, 130, 150, 12000, 22),
    cho('Szeroki syntezator', 1, 0.5, 55, 14, 0, 180, 20, 16000, 50),
    cho('Vintage — dwa BBD', 0, 0.5, 70, 5, 0, 160, 80, 7000, 50),
    cho('Czysta gitara lat 80', 0, 0.9, 45, 8, 15, 140, 60, 10000, 45),
    cho('Bas — stabilny dół', 0, 0.45, 35, 10, 0, 120, 250, 9000, 45),
    cho('Smyczki — ensemble', 1, 0.7, 35, 18, 0, 160, 100, 9000, 60),
    cho('Wir z tempem (1 takt)', 1, 0.6, 50, 15, -30, 200, 120, 14000, 45, { sync: 1, div: 4 }),
    cho('Vibrato', 2, 5.5, 35, 12, 0, 0, 20, 20000, 100),
  ],
  phaser: [
    pha('Klasyczny, 4 stopnie', 1, 0.5, 900, 70, 0, 90, 50),
    pha('Delikatny', 0, 0.2, 1200, 45, 10, 60, 35),
    pha('Głębokie przemiatanie', 4, 0.08, 700, 90, 45, 120, 50),
    pha('Funk — gitara (1/8)', 2, 0.4, 1100, 60, 55, 0, 50, { sync: 1, div: 1 }),
    pha('Odrzutowiec', 4, 0.05, 1500, 85, 85, 30, 50),
    pha('Wir stereo', 3, 0.3, 800, 75, 35, 180, 50),
    pha('Pusty (ujemny feedback)', 2, 0.35, 600, 70, -60, 90, 50),
    pha('Pad — przemiatanie (2 takty)', 3, 0.4, 900, 80, 40, 90, 50, { sync: 1, div: 5 }),
  ],
  eq: [
    eqp('Flat', 0, []),
    eqp('Wokal — obecność', 0, [[0, T, 100, n, n], [2, n, 300, -2.5, 1.2], [4, n, 3500, 3, 0.9], [5, n, 10000, 2.5, n]]),
    eqp('Kick — punch', 0, [[0, T, 30, n, n], [1, n, 60, 3, n], [2, n, 350, -4, 1.4], [4, n, 4000, 3, 1.2]]),
    eqp('Bas — czysty dół', 0, [[0, T, 35, n, n], [1, n, 80, 2, n], [2, n, 250, -3, n], [6, T, 8000, n, n]]),
    eqo('Mud cut', 0, 12, [[0, T, 60, n, n], [2, n, 300, -4, 1.5]]),
    eqp('Air / połysk', 0, [[4, n, 5000, 1, n], [5, n, 12000, 4, n]]),
    eqo('Pasmo — wytnij dół i górę', 0, 24, [[0, T, 120, n, n], [6, T, 9000, n, n]]),
    eqo('Telefon / lo-fi', 2, 24, [[0, T, 400, n, n], [3, n, 1500, 4, 0.8], [6, T, 3200, n, n]]),
    eqp('Master — smile', 0, [[1, n, 80, 1.5, n], [3, n, 1000, -0.5, 0.6], [5, n, 10000, 1.5, n]]),
    eqo('Wytnij < 20 Hz i > 18 kHz', 0, 24, [[0, T, 20, n, n], [6, T, 18000, n, n]]),
    eqo('Low cut 80 Hz', 0, 24, [[0, T, 80, n, n]]),
    eqo('Low cut 150 Hz (wokal)', 0, 24, [[0, T, 150, n, n]]),
    eqo('High cut 10 kHz', 0, 24, [[6, T, 10000, n, n]]),
  ],
  comp: [
    pr('Domyślny', { thr: -18, ratio: 4, att: 10, rel: 150, gain: 0 }),
    pr('Wokal — wyrównanie', { thr: -20, ratio: 3, att: 5, rel: 120, gain: 4 }),
    pr('Drum bus — glue', { thr: -14, ratio: 2, att: 30, rel: 200, gain: 2 }),
    pr('Kick — punch', { thr: -16, ratio: 4, att: 25, rel: 80, gain: 3 }),
    pr('Bas — kontrola', { thr: -22, ratio: 5, att: 3, rel: 90, gain: 4 }),
    pr('Delikatny', { thr: -12, ratio: 1.5, att: 20, rel: 250, gain: 1 }),
    pr('Smash', { thr: -35, ratio: 12, att: 0.5, rel: 60, gain: 12 }),
  ],
  sat: [
    pr('Domyślny', { style: 0, drive: 40 }),
    pr('Wokal — ciepło', { style: 0, drive: 35, lowcut: 80, tone: 10, highcut: 16000 }),
    pr('Wokal — lampa', { style: 3, drive: 45, lowcut: 70, highcut: 15000 }),
    pr('Rap — ziarno', { style: 1, drive: 58, lowcut: 100, tone: 25, highcut: 14000 }),
    pr('Bębny — smash', { style: 1, drive: 80, lowcut: 30, thump: 30, mix: 70 }),
    pr('Bas — waga', { style: 2, drive: 50, lowcut: 35, thump: 40, tone: -20 }),
    pr('Lo-fi głośnik', { style: 4, drive: 60, lowcut: 300, thump: 35, tone: 20, highcut: 4500, steep: 1 }),
    pr('Równoległe zgniecenie', { style: 4, drive: 90, punish: 1, mix: 30 }),
    pr('Taśma — klej na bus', { style: 0, drive: 22, highcut: 18000 }),
  ],
  delay: [
    dly('Ósemka 1/8', 0.5, 35, 22),
    dly('Ping-pong 1/8', 0.5, 50, 30, { model: 2, pan: -70, cutoff: 7000 }), dly('Ping-pong kropkowana 1/8', 0.75, 50, 30, { model: 2, pan: -70, cutoff: 7000 }),
    dly('Ping-pong 1/4', 1, 45, 28, { model: 2, pan: -70, cutoff: 6000 }),
    dly('Kropkowana 1/8', 0.75, 40, 22), dly('Ćwierćnuta 1/4', 1, 30, 22), dly('Triola 1/8T', 1 / 3, 35, 20),
    dlyMs('Slapback', 90, 0, 25, { ftype: 1, cutoff: 7000 }),
    dly('Wysyłka — 100 % wet', 0.75, 40, 100, { dry: 0, tone: 25 }),
    dly('Taśma — dub', 1.5, 70, 30, { cutoff: 2400, res: 20, level: -10, knee: 70, sym: 30, mtime: 18, rate: 0.6, smooth: 65, diff: 15 }),
    dly('Dub — samooscylacja', 0.75, 108, 28, { cutoff: 1200, res: 55, level: -9, knee: 80, smooth: 80 }),
    dly('Lo-fi / bitcrush', 0.5, 45, 30, { ftype: 0, srate: 8000, bits: 8, level: -6 }),
    dly('Rozmyte echo (ambient)', 1, 60, 30, { cutoff: 4500, diff: 70, dspread: 70, mtime: 10, rate: 0.3 }),
    dlyMs('Chorus / dublowanie', 14, 15, 45, { ftype: 0, offset: 40, mtime: 55, rate: 0.7, keep: 1 }),
    dlyMs('Multi-FX: filtr + drive (bez echa)', 100, 0, 100, { model: 3, dry: 0, cutoff: 1800, res: 55, level: -14, mcut: 45, rate: 0.25 }),
  ],
  reverb: ([
    ['Vocal Plate', 1.0, 25, 300, 7000, 100, 15, 0, 0, 3, 0.8], ['Vocal Room', 0.6, 10, 250, 8000, 90, 12, -10, 1, 0, 0.9],
    ['Vocal Hall', 2.0, 35, 250, 6500, 100, 14, -20, 2, 4, 0.8], ['Ambience', 0.4, 0, 200, 9000, 100, 12, 10, 3, 0, 1.0],
    ['Ballada — długi plate', 2.4, 40, 350, 6000, 100, 18, -30, 0, 6, 0.8], ['Bębny — pokój', 0.7, 0, 150, 9000, 80, 12, 0, 1, 0, 1.0],
    ['Wysyłka — hala 100 %', 2.2, 20, 200, 7000, 100, 100, -10, 2, 0, 0.9],
    ['Wokal — rap, krótko', 0.5, 10, 400, 7500, 80, 10, 0, 1, 5, 0.7], ['Wokal — R&B, hala', 2.6, 45, 250, 6500, 100, 16, -20, 2, 5, 0.8],
    ['Wokal — pop, jasny plate', 1.8, 60, 350, 9000, 100, 18, 25, 0, 6, 0.7], ['Werbel — plate', 1.1, 0, 250, 9000, 100, 20, 10, 0, 0, 0.9],
    ['Klawisze — ciepła hala', 2.2, 20, 200, 5500, 100, 22, -30, 2, 0, 1.1], ['Lead — EDM, hala', 3.5, 30, 300, 9000, 100, 25, 10, 2, 6, 0.9],
    ['Pad — ambientowa mgła', 8.0, 60, 200, 7000, 100, 35, -10, 2, 0, 1.0, 80, 20], ['Lo-fi — ciemny pokój', 1.0, 15, 300, 3500, 100, 25, -60, 1, 0, 1.0],
    ['Kosmos — 18 s', 18, 80, 250, 6000, 100, 40, -20, 2, 0, 0.9, 100, 10], ['Wysyłka — plate 100 %', 1.5, 20, 300, 8000, 100, 100, 0, 0, 0, 0.8],
  ] as [string, ...number[]][]).map(([name, size, pre, lo, hi, width, mix, tone, space, duck, bass, mod, early]) =>
    pr(name, { size, pre, lo, hi, width, mix, tone, space, duck, early: early ?? 50, bass, mod: mod ?? 50 })),
  limit: [
    pr('Bezpieczny', { gain: 0, ceil: -1 }), pr('Streaming −1 dBTP', { gain: 4, ceil: -1, rel: 80 }),
    pr('Głośny master', { gain: 7, ceil: -1, rel: 120 }), pr('Bardzo głośny', { gain: 10, ceil: -0.5, rel: 150, look: 5 }),
    pr('Przezroczysty', { gain: 3, ceil: -1, rel: 250, look: 5 }),
  ],
  fet: [
    pr('Wokal — klasyk', { input: 8, output: -2, ratio: 0, attack: 400, release: 250, mix: 100 }),
    pr('Wokal — z przodu', { input: 12, output: -4, ratio: 1, attack: 200, release: 150, mix: 100 }),
    pr('Bas — kontrola', { input: 10, output: -3, ratio: 2, attack: 600, release: 400, mix: 100 }),
    pr('Równoległa kompresja', { input: 18, output: -6, ratio: 4, attack: 100, release: 120, mix: 45 }),
    pr('All buttons — smash', { input: 20, output: -10, ratio: 4, attack: 60, release: 100, mix: 100 }),
  ],
  deess: [
    pr('Wokal — domyślny', { thr: -32, range: 8, freq: 6000, mode: 0, look: 5, listen: 0 }),
    pr('Ostre „s”', { thr: -36, range: 12, freq: 6500, mode: 0, look: 8, listen: 0 }),
    pr('Delikatnie', { thr: -26, range: 5, freq: 7000, mode: 0, look: 5, listen: 0 }),
    pr('Wokal żeński', { thr: -32, range: 8, freq: 8000, mode: 0, look: 6, listen: 0 }),
    pr('Wokal męski', { thr: -32, range: 8, freq: 5000, mode: 0, look: 6, listen: 0 }),
    pr('Szerokopasmowy', { thr: -30, range: 6, freq: 6000, mode: 1, look: 5, listen: 0 }),
  ],
  denoise: [
    pr('Wokal — delikatnie', { reduction: 8, sens: 35, smooth: 50 }),
    pr('Wokal — szum pokoju', { reduction: 14, sens: 50, smooth: 60 }),
    pr('Telefon / dyktafon', { reduction: 20, sens: 65, smooth: 70 }),
    pr('Mocno', { reduction: 30, sens: 80, smooth: 80 }),
  ],
  tune: [
    pr('Naturalny', { retune: 40, humanize: 30, flex: 40, vibrato: 60 }),
    pr('Delikatna korekta', { retune: 80, humanize: 50, flex: 70, vibrato: 80 }),
    pr('Pop — mocno', { retune: 10, humanize: 10, flex: 10, vibrato: 30 }),
    pr('Efekt robota', { retune: 0 }),
  ],
  voice: [
    pr('Domyślny', {}), pr('Głębszy, pełniejszy', { formant: -2 }),
    pr('Oktawa w dół', { pitch: -12, formant: -2 }), pr('Oktawa w górę', { pitch: 12, formant: 2 }),
    pr('Zmiana płci ↓', { pitch: -4, formant: -4 }), pr('Zmiana płci ↑', { pitch: 4, formant: 4 }),
    pr('Chipmunk', { pitch: 7, link: 1 }), pr('Demon', { pitch: -7, formant: -6, drive: 12, out: -4 }),
    pr('Hard tune + drive', { mode: 1, drive: 10, out: -3 }), pr('Robot', { mode: 2, drive: 6, out: -2 }),
  ],
  rider: [
    pr('Wokal — śpiew', { amount: 50, phrase: 20, up: 5, down: 6, speed: 0, auto: 1, look: 2 }),
    pr('Rap', { amount: 50, phrase: 20, up: 5, down: 6, speed: 1, auto: 1, look: 2 }),
    pr('Delikatnie', { amount: 30, phrase: 10, up: 3, down: 4, speed: 0, auto: 1, look: 2 }),
    pr('Mocno', { amount: 80, phrase: 30, up: 8, down: 9, speed: 0, auto: 1, look: 2 }),
    pr('Wokal — śpiew (długi look-ahead, opóźnienie)', { amount: 50, phrase: 20, up: 5, down: 6, speed: 0, auto: 1, look: 1 }),
  ],
  warp: [
    pr('Domyślny', {}), pr('Half-time', { tslot: 1 }), pr('Sidechain 1/4', { vslot: 1 }), pr('Tape stop (pół taktu)', { tslot: 8 }),
    pr('Stutter 1/8 + bramka', { tslot: 5, vslot: 4 }), pr('Wyzwalanie na żywo (co ćwierćnutę, one-shot)', { trig: 3, oneshot: 1 }),
  ],
  half: ([
    ['Half-time — 1 takt', 1, 4, 10, 20, 20000, 0, 100], ['Bębny — ósemki, punch', 1, 1, -12, 20, 20000, 0, 100],
    ['Szesnastki — oktawa w dół', 1, 0, -8, 20, 20000, 0, 100], ['Triola — kwinta niżej', 0, 2, 10, 20, 20000, 0, 100],
    ['Kwinta — harmonia 50 %', 0, 3, 20, 20, 20000, 0, 50], ['4× — tekstura', 2, 4, 45, 20, 20000, 0, 100],
    ['Swell — od tyłu', 1, 3, 80, 20, 20000, 0, 100], ['FX — swoop', 1, 2, -70, 20, 20000, 0, 100],
    ['Tylko góra (sub zostaje)', 1, 4, 10, 150, 20000, 0, 100], ['Środek pasma — solo', 1, 2, 10, 300, 4000, 1, 100],
  ] as [string, ...number[]][]).map(([name, speed, loop, smooth, lo, hi, solo, mix]) => pr(name, { speed, loop, smooth, lo, hi, solo, mix })),
};

/** catalog.rs `BUILTIN` (also the order of the "add effect" menu). */
export const BUILTIN: AnyBuiltinKind[] = ['denoise', 'tune', 'voice', 'eq', 'rider', 'deess', 'comp', 'fet', 'multiband', 'sat', 'delay', 'half', 'warp', 'chorus', 'phaser', 'stereo', 'reverb', 'limit', 'analyzer'];

export function mockCatalog(): Catalog {
  return {
    types: Object.fromEntries(BUILTIN.map(k => [k, { name: NAMES[k], params: PARAMS[k] }])),
    presets: Object.fromEntries(BUILTIN.map(k => [k, PRESETS[k]])),
    synth: structuredClone(SYNTH_CATALOG),
    warp: warpFactory(),
    warpLibrary: (({ time, vol }) => ({ time, vol }))(JSON.parse(warpLibraryJson)),
  };
}

const TC = ['#4f8ef7', '#5aa9e6', '#69c3d6', '#47b8a8', '#5fbf8a', '#7c8cf8', '#e0a44a', '#9a8cf0', '#d9678f', '#8b93a1'];
const COLORS = [0, 6, 5, 3, 4, 8, 9, 9];

function base(names: string[], insNames: string[], cat: Catalog): Project {
  const p: Project = {
    version: 3, bpm: 128, tracks: [], patterns: [], sources: [], clips: [], loop: null, nextId: 1000, automations: [],
    inserts: [{ id: 0, name: 'Master', color: '#d5dae2', vol: VOL_UNITY, pan: 0, width: 1, muted: false, solo: false, fx: [] }],
  };
  insNames.forEach((name, i) => p.inserts.push({ id: i + 1, name, color: TC[COLORS[i]], vol: VOL_UNITY, pan: 0, width: 1, muted: false, solo: false, fx: [] }));
  names.forEach((name, i) => p.tracks.push({ id: i + 1, name, color: TC[COLORS[i]], muted: false, solo: false, arm: false, vol: VOL_UNITY, h: 56, insert: i + 1 }));
  p.inserts[0].fx.push(newFx(cat, 900, 'limit', p.bpm));
  return p;
}

/**
 * A sketch of a song (demo.rs `sketch_project`): three short patterns on the starter kit plus
 * a gated 808 bass, a vocal chain and a drum-bus compressor. Small and fixed: tests build on it.
 */
export function sketchProject(cat: Catalog): Project {
  const p = base(['Drums', 'Hats', 'Bass', 'Pad', 'Perc', 'Vocal', 'Track 7', 'Track 8'],
    ['Drums', 'Hats', 'Bass', 'Pad', 'Perc', 'Vocal', 'Insert 7', 'Insert 8'], cat);
  p.tracks[5].arm = true;
  const [kick, clap, hat] = addStarterKit(p);
  const bass = factoryChannel(p, 'bass-808.flac')!;
  const b = p.channels!.find(c => c.id === bass)!;
  b.name = 'Bass';
  if (b.kind === 'sample') b.sampler.env = true;
  const note = (ch: number, step: number, len: number, key?: number): Note => ({ ch, step, len, p: key, trig: false });
  p.patterns = [
    { id: 1, name: 'Beat', color: '#4f8ef7', len: 4,
      notes: ([[0, kick], [4, kick], [8, kick], [12, kick], [10, kick], [4, clap], [12, clap]] as [number, number][]).map(([s, c]) => ({ ...note(c, s, 1), trig: true })) },
    { id: 2, name: 'Hats', color: '#e0a44a', len: 4, notes: [2, 6, 10, 14, 15, 7].map(s => ({ ...note(hat, s, 1), trig: true })) },
    { id: 3, name: 'Bass', color: '#7c8cf8', len: 8,
      notes: [[0, 3, 33], [4, 2, 33], [6, 2, 40], [8, 4, 36], [14, 2, 38], [16, 3, 29], [20, 2, 29], [22, 2, 36], [24, 6, 31], [30, 2, 34]].map(([s, l, k]) => note(bass, s, l, k)) },
  ];
  const clip = (id: number, track: number, start: number, len: number, ref: number): Clip =>
    ({ id, track, start, len, offset: 0, kind: 'pat', ref, muted: false, gain: 0, fadeIn: 0, fadeOut: 0, pan: 0, norm: false, rev: false });
  p.clips = [clip(1, 0, 0, 16, 1), clip(2, 0, 16, 16, 1), clip(3, 1, 8, 24, 2), clip(4, 2, 0, 32, 3)];
  const eq = loadPreset(cat, newFx(cat, 902, 'eq', p.bpm), 1, p.bpm);
  p.inserts[6].fx = [eq, newFx(cat, 903, 'comp', p.bpm), newFx(cat, 904, 'reverb', p.bpm)];
  p.inserts[1].fx = [loadPreset(cat, newFx(cat, 905, 'comp', p.bpm), 2, p.bpm)];
  return p;
}
