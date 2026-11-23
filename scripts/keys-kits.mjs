// Builds the Synkopa Keys instrument kits: for every kit below it downloads the recordings it
// plays (pinned to a commit), records each file's SHA-256 and size in assets-manifest.json,
// and writes the instrument's SFZ mapping into crates/engine/instruments/. Run it when an
// instrument is added or changed; the app only reads the results.
//   node scripts/keys-kits.mjs [kit-id ...]
// Downloads are cached in target/keys-kits/<kit>/ (checked against the manifest on reuse).
import { createHash } from 'node:crypto';
import { mkdir, readFile, rename, stat, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const instDir = join(root, 'crates', 'engine', 'instruments');
const raw = (repo, commit, path) =>
  `https://raw.githubusercontent.com/${repo}/${commit}/${path.split('/').map(encodeURIComponent).join('/')}`;

const CC0 = { license: 'CC0-1.0', license_url: 'https://creativecommons.org/publicdomain/zero/1.0/' };
const CCBY3 = { license: 'CC-BY-3.0', license_url: 'https://creativecommons.org/licenses/by/3.0/' };
const VSCO = { repo: 'sgossner/VSCO-2-CE', commit: '440300901dfe9275fd84e0b7763af1f8443ae62e', author: 'Versilian Studios LLC', ...CC0 };
const VCSL = { repo: 'sgossner/VCSL', commit: 'c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e', author: 'Versilian Studios LLC', ...CC0 };
const GSEP = { repo: 'sfzinstruments/GregSullivan.E-Pianos', commit: '8c3e581acda3594b553948ff0222d4f84a698376', author: 'Greg Sullivan', ...CCBY3 };
const SAL = { repo: 'sfzinstruments/SalamanderGrandPiano', commit: '3382bf9496bba2486f5ab0de55a264d1dfc38404', author: 'Alexander Holm', ...CCBY3 };

const NOTE = { c: 0, d: 2, e: 4, f: 5, g: 7, a: 9, b: 11 };
/** `F#3` → MIDI key, `c4offset` = the key the file names call C4 (60 scientific, 48 when C3 = middle C (C4 is key 72 in their names)). */
const key = (name, c4 = 60) => {
  const m = /^([A-Ga-g])(#|b)?(-?\d)$/.exec(name);
  if (!m) throw new Error(`not a note: ${name}`);
  return c4 + (Number(m[3]) - 4) * 12 + NOTE[m[1].toLowerCase()] + (m[2] === '#' ? 1 : m[2] === 'b' ? -1 : 0);
};

/** Key ranges for sampled roots: each root covers up to halfway to its neighbours. */
function spans(roots, lo, hi) {
  const r = [...new Set(roots)].sort((a, b) => a - b);
  return r.map((k, i) => ({
    root: k,
    lo: i === 0 ? Math.min(lo, k) : Math.floor((r[i - 1] + k) / 2) + 1,
    hi: i === r.length - 1 ? Math.max(hi, k) : Math.floor((k + r[i + 1]) / 2),
  }));
}

const header = (title, credit, src, notes = []) =>
  [`// Synkopa Keys: ${title}.`, `// ${credit}, ${src.license}.`, `// https://github.com/${src.repo} (${src.commit.slice(0, 10)})`, ...notes.map(n => `// ${n}`), ''].join('\n');

// ------------------------------------------------------------------ the kits

let treeCache;
/** Every file path of VSCO 2 at the pinned commit: one GitHub API call, cached in
 *  target/keys-kits/vsco-tree.json (the recursive listing of this large repo can time out;
 *  GITHUB_TOKEN, if set, is sent with it). */
async function vscoTree() {
  if (treeCache) return treeCache;
  const cache = join(root, 'target', 'keys-kits', `vsco-tree-${VSCO.commit.slice(0, 10)}.json`);
  try {
    treeCache = JSON.parse(await readFile(cache, 'utf8'));
    return treeCache;
  } catch { /* not cached */ }
  const headers = process.env.GITHUB_TOKEN ? { authorization: `Bearer ${process.env.GITHUB_TOKEN}` } : {};
  const res = await fetch(`https://api.github.com/repos/${VSCO.repo}/git/trees/${VSCO.commit}?recursive=1`, { headers });
  if (!res.ok) throw new Error(`VSCO tree: HTTP ${res.status}`);
  treeCache = (await res.json()).tree.filter(t => t.type === 'blob').map(t => t.path);
  await mkdir(dirname(cache), { recursive: true });
  await writeFile(cache, JSON.stringify(treeCache));
  return treeCache;
}

const KITS = [
  {
    id: 'salamander-piano',
    name: 'Salamander Grand Piano',
    description: 'Yamaha C5 grand for Synkopa Keys: 30 notes x 4 dynamics, 88 key-release noises',
    attribution: 'Salamander Grand Piano V3 by Alexander Holm, licensed CC-BY-3.0. Source: https://github.com/sfzinstruments/SalamanderGrandPiano',
    source: SAL,
    // the mapping is hand-tuned (crates/engine/instruments/grand-piano.sfz): only the files
    files: () => {
      const notes = ['A0', 'C1', 'D#1', 'F#1', 'A1', 'C2', 'D#2', 'F#2', 'A2', 'C3', 'D#3', 'F#3', 'A3', 'C4', 'D#4', 'F#4', 'A4', 'C5', 'D#5', 'F#5', 'A5', 'C6', 'D#6', 'F#6', 'A6', 'C7', 'D#7', 'F#7', 'A7', 'C8'];
      const f = [];
      for (const l of [1, 6, 11, 16]) for (const n of notes) f.push({ file: `${n}v${l}.flac`, path: `Samples/${n}v${l}.flac` });
      for (let i = 1; i <= 88; i++) f.push({ file: `rel${i}.flac`, path: `Samples/rel${i}.flac` });
      return { files: f };
    },
  },
  {
    id: 'e-pianos',
    name: 'Electric Pianos',
    description: 'Wurlitzer EP200, Yamaha CP80 and Hohner Pianet T for Synkopa Keys',
    attribution: "Greg Sullivan's E-Pianos (Wurlitzer EP200, Yamaha CP80, Hohner Pianet T) by Greg Sullivan, SFZ mapping by kinwie, licensed CC-BY-3.0. Source: https://github.com/sfzinstruments/GregSullivan.E-Pianos",
    source: GSEP,
    files: async () => {
      const files = [];
      const sfz = {};
      // `trim` (dB) levels each with the pianos: keys_demo plays them all the same phrase
      for (const [id, dir, file, prefix, title, trim] of [
        ['wurlitzer', 'Wurlitzer EP200', 'Wurlitzer EP200.sfz', 'wurli-', 'Wurlitzer EP200', 4.5],
        ['cp80', 'CP80', 'CP80.sfz', 'cp80-', 'Electric Grand CP80', 4],
        ['pianet', 'Pianet T', 'Pianet T.sfz', 'pianet-', 'Pianet T', 2.5],
      ]) {
        const res = await fetch(raw(GSEP.repo, GSEP.commit, `${dir}/${file}`));
        if (!res.ok) throw new Error(`${dir}/${file}: HTTP ${res.status}`);
        let text = await res.text();
        const ext = /#define \$EXT (\S+)/.exec(text)?.[1] ?? 'flac';
        // the kit's files are flat: the default path becomes the instrument's file prefix
        text = text.replace(/default_path=Samples\//, `default_path=${prefix}`);
        text = text.replace(/<global>\r?\n/, `<global>\nglobal_volume=${trim}\n`);
        // their release knob's default (0.1 s) gates chords off; 0.3 s lets them ring out
        text = text.replace('set_hdcc$RELEASE=0.05', 'set_hdcc$RELEASE=0.15');
        for (const m of text.matchAll(/sample=(\S+)/g)) {
          const name = m[1].replace('$EXT', ext);
          if (!files.some(f => f.file === prefix + name)) files.push({ file: prefix + name, path: `${dir}/Samples/${name}` });
        }
        sfz[id] = header(title, `${title} by Greg Sullivan, mapping by kinwie`, GSEP, [
          "The original mapping, with its sample folder renamed to the kit's flat files and a",
          'global_volume that levels it with the other instruments.',
        ]) + text;
      }
      return { files, sfz };
    },
  },
  {
    id: 'upright-piano',
    name: 'Upright Piano',
    description: 'VSCO 2 upright piano for Synkopa Keys: C and G of every octave, 3 dynamics, 2 round robins',
    attribution: 'Upright Nr1 from VSCO 2 Community Edition by Versilian Studios LLC, released to the public domain under CC0-1.0.',
    source: VSCO,
    files: () => {
      const dir = 'Keys/Upright Nr1';
      const names = `UR1_C1_f_RR1 UR1_C1_f_RR2 UR1_C1_mf_RR2 UR1_C1_pp_RR1 UR1_C1_pp_RR2 UR1_C2_f_RR1 UR1_C2_f_RR2 UR1_C2_mf_RR1 UR1_C2_mf_RR2 UR1_C2_pp_RR1 UR1_C3_f_RR1 UR1_C3_f_RR2 UR1_C3_mf_RR1 UR1_C3_mf_RR2 UR1_C3_pp_RR1 UR1_C3_pp_RR2 UR1_C4_f_RR1 UR1_C4_f_RR2 UR1_C4_mf_RR1 UR1_C4_mf_RR2 UR1_C4_pp_RR1 UR1_C4_pp_RR2 UR1_C5_f_RR1 UR1_C5_f_RR2 UR1_C5_mf_RR1 UR1_C5_mf_RR2 UR1_C5_pp_RR1 UR1_C5_pp_RR2 UR1_C6_f_RR1 UR1_C6_f_RR2 UR1_C6_mf_RR1 UR1_C6_mf_RR2 UR1_C6_pp_RR1 UR1_C6_pp_RR2 UR1_C7_f_RR1 UR1_C7_f_RR2 UR1_C7_mf_RR1 UR1_C7_mf_RR2 UR1_C7_pp_RR1 UR1_C7_pp_RR2 UR1_G1_f_RR2 UR1_G1_mf_RR1 UR1_G1_mf_RR2 UR1_G1_pp_RR1 UR1_G2_f_RR1 UR1_G2_f_RR2 UR1_G2_mf_RR1 UR1_G2_mf_RR2 UR1_G2_pp_RR1 UR1_G3_f_RR1 UR1_G3_f_RR2 UR1_G3_mf_RR1 UR1_G3_mf_RR2 UR1_G3_pp_RR1 UR1_G3_pp_RR2 UR1_G4_f_RR1 UR1_G4_f_RR2 UR1_G4_mf_RR1 UR1_G4_mf_RR2 UR1_G4_pp_RR1 UR1_G4_pp_RR2 UR1_G5_f_RR1 UR1_G5_f_RR2 UR1_G5_mf_RR1 UR1_G5_mf_RR2 UR1_G5_pp_RR1 UR1_G5_pp_RR2 UR1_G6_f_RR1 UR1_G6_f_RR2 UR1_G6_mf_RR1 UR1_G6_mf_RR2 UR1_G6_pp_RR1 UR1_G6_pp_RR2 UR1_G7_f_RR1 UR1_G7_f_RR2 UR1_G7_mf_RR1 UR1_G7_mf_RR2 UR1_G7_pp_RR1 UR1_G7_pp_RR2`.split(' ');
      const parsed = names.map(n => {
        const [, note, dyn, rr] = /^UR1_([A-G]\d)_(pp|mf|f)_RR(\d)$/.exec(n);
        return { n, k: key(note), dyn, rr: Number(rr) };
      });
      const files = names.map(n => ({ file: `${n}.wav`, path: `${dir}/${n}.wav` }));
      const sp = spans(parsed.map(p => p.k), 21, 108);
      const layers = [['pp', 1, 50], ['mf', 51, 95], ['f', 96, 127]];
      let o = header('Upright Piano', 'Upright Nr1 from VSCO 2 Community Edition by Versilian Studios', VSCO, [
        'Mapping by Synkopa: three dynamics, two round robins where both were recorded.',
      ]);
      o += '\n<global> ampeg_release=0.35 amp_veltrack=70\n';
      for (const [dyn, lo, hi] of layers) {
        o += `\n<group> lovel=${lo} hivel=${hi}\n`;
        for (const s of sp) {
          const takes = parsed.filter(p => p.k === s.root && p.dyn === dyn).sort((a, b) => a.rr - b.rr);
          // a dynamic missing at this note borrows the nearest recorded one
          const use = takes.length ? takes : parsed.filter(p => p.k === s.root && p.dyn === (dyn === 'mf' ? 'f' : 'mf'));
          use.forEach((t, i) => {
            const rr = use.length > 1 ? ` seq_length=${use.length} seq_position=${i + 1}` : '';
            o += `<region> sample=${t.n}.wav lokey=${s.lo} hikey=${s.hi} pitch_keycenter=${s.root}${rr}\n`;
          });
        }
      }
      return { files, sfz: { 'upright-piano': o } };
    },
  },
  {
    id: 'string-section',
    name: 'String Section',
    description: 'VSCO 2 violin, viola and cello sections for Synkopa Keys: sustain, tremolo, pizzicato, spiccato',
    attribution: 'Violin, viola and cello sections from VSCO 2 Community Edition by Versilian Studios LLC, released to the public domain under CC0-1.0.',
    source: VSCO,
    files: async () => {
      const tree = await vscoTree();
      // VSCO names its notes with C3 = middle C (C4 is key 72 in their names)
      const sections = [
        { name: 'cello', root: 'Strings/Cello Section', dirs: { sustain: 'susvib', tremolo: 'trem', pizzicato: 'pizzT', spiccato: 'spic' }, keys: [33, 50], volume: 0 },
        { name: 'viola', root: 'Strings/Viola Section', dirs: { sustain: 'susvib', tremolo: 'trem', pizzicato: 'pizz', spiccato: 'spic' }, keys: [51, 59], volume: -1 },
        { name: 'violin', root: 'Strings/Violin Section', dirs: { sustain: 'susVib', tremolo: 'Trem', pizzicato: 'Pizz', spiccato: 'Spic' }, keys: [60, 93], volume: -1 },
      ];
      // keyswitch, label, envelope: bowed articulations swell in and ring out, plucked and
      // bounced ones play their recording
      const arts = [
        ['sustain', 24, 'Sustain', 'ampeg_attack=0.04 ampeg_release=1.4 global_volume=2.5'],
        ['tremolo', 25, 'Tremolo', 'ampeg_attack=0.03 ampeg_release=1.2'],
        ['pizzicato', 26, 'Pizzicato', 'ampeg_release=0.5 global_volume=7'],
        ['spiccato', 27, 'Spiccato', 'ampeg_release=0.2 global_volume=8'],
      ];
      const files = [];
      let o = header('String Section', 'Violin, viola and cello sections from VSCO 2 Community Edition by Versilian Studios', VSCO, [
        'Mapping by Synkopa: four articulations on keyswitches C0..D#0 (keys 24..27), each with the',
        'cellos low, the violas in the middle and the violins on top (split, not layered: two',
        'sections on one note comb-filter), two dynamics split at velocity 80, round robins where',
        'they were recorded.',
      ]);
      o += '\n<global> sw_lokey=24 sw_hikey=27 sw_default=24 amp_veltrack=40 global_volume=1.5\n';
      for (const [art, sw, label, env] of arts) {
        o += `\n<master> sw_last=${sw} sw_label=${label} ${env}\n`;
        for (const s of sections) {
          const dir = `${s.root}/${s.dirs[art]}`;
          const parsed = tree
            .filter(p => p.startsWith(dir + '/') && p.endsWith('.wav'))
            .map(p => {
              const n = p.slice(dir.length + 1, -4);
              const m = /_([A-G]#?\d)_v(\d)(?:_(?:rr|RR)?(\d+))?$/.exec(n);
              if (!m) throw new Error(`${dir}: ${n}`);
              return { n, path: p, k: key(m[1], 72), v: Number(m[2]), rr: Number(m[3] ?? 1) };
            });
          const prefix = `${s.name}-${art}-`;
          const dyns = [...new Set(parsed.map(p => p.v))].sort();
          const sp = spans(parsed.map(p => p.k), s.keys[0], s.keys[1]).filter(r => r.lo <= s.keys[1] && r.hi >= s.keys[0]);
          [dyns[0], dyns[dyns.length - 1]].forEach((d, i) => {
            if (i === 1 && dyns.length === 1) return;
            const vel = dyns.length === 1 ? '' : i === 0 ? ' lovel=1 hivel=79' : ' lovel=80 hivel=127';
            o += `<group>${vel} volume=${s.volume}\n`;
            for (const r of sp) {
              const lo = Math.max(r.lo, s.keys[0]);
              const hi = Math.min(r.hi, s.keys[1]);
              let takes = parsed.filter(p => p.k === r.root && p.v === d);
              if (!takes.length) takes = parsed.filter(p => p.k === r.root);
              takes.sort((a, b) => a.rr - b.rr);
              takes.forEach((t, j) => {
                const rr = takes.length > 1 ? ` seq_length=${takes.length} seq_position=${j + 1}` : '';
                // only the recordings a region plays go into the kit
                if (!files.some(f => f.path === t.path)) files.push({ file: `${prefix}${t.n}.wav`, path: t.path });
                o += `<region> sample=${prefix}${t.n}.wav lokey=${lo} hikey=${hi} pitch_keycenter=${r.root}${rr}\n`;
              });
            }
          });
        }
      }
      return { files, sfz: { strings: o } };
    },
  },
  {
    id: 'harp',
    name: 'Concert Harp',
    description: 'VSCO 2 concert harp for Synkopa Keys: 23 notes across seven octaves',
    attribution: 'Harp from VSCO 2 Community Edition by Versilian Studios LLC, released to the public domain under CC0-1.0.',
    source: VSCO,
    files: () => {
      const names = 'KSHarp_A2_mf KSHarp_A4_mf KSHarp_A6_mf KSHarp_B1_mf KSHarp_B3_mf KSHarp_B5_mf KSHarp_B6_mf KSHarp_C3_mf KSHarp_C5_mf KSHarp_D2_mf KSHarp_D4_mf KSHarp_D6_mf KSHarp_D7_f KSHarp_E1_f KSHarp_E3_mf KSHarp_E5_mf KSHarp_F2_mf KSHarp_F4_mf KSHarp_F6_mf KSHarp_F7_f KSHarp_G1_mp KSHarp_G3_mf KSHarp_G5_mf'.split(' ');
      const parsed = names.map(n => ({ n, k: key(/^KSHarp_([A-G]\d)_/.exec(n)[1]) }));
      const files = names.map(n => ({ file: `${n}.wav`, path: `Strings/Harp/${n}.wav` }));
      let o = header('Concert Harp', 'Harp from VSCO 2 Community Edition by Versilian Studios', VSCO, ['Mapping by Synkopa.']);
      o += '\n<global> loop_mode=one_shot amp_veltrack=85 global_volume=18\n<group>\n';
      for (const r of spans(parsed.map(p => p.k), 21, 105)) {
        o += `<region> sample=${parsed.find(p => p.k === r.root).n}.wav lokey=${r.lo} hikey=${r.hi} pitch_keycenter=${r.root}\n`;
      }
      return { files, sfz: { harp: o } };
    },
  },
  {
    id: 'solo-violin',
    name: 'Solo Violin',
    description: 'VSCO 2 solo violin for Synkopa Keys: arco with vibrato, tremolo, pizzicato, spiccato',
    attribution: 'Solo violin from VSCO 2 Community Edition by Versilian Studios LLC, released to the public domain under CC0-1.0.',
    source: VSCO,
    files: async () => {
      const tree = await vscoTree();
      // the arco and pizzicato files name C4 as middle C, tremolo and spiccato C3 (checked by pitch)
      const arts = [
        ['Arco Vib', 24, 'Arco', 60, 'ampeg_attack=0.02 ampeg_release=0.6 global_volume=6'],
        ['Trem', 25, 'Tremolo', 72, 'ampeg_attack=0.02 ampeg_release=0.6 global_volume=5'],
        ['Pizz', 26, 'Pizzicato', 60, 'ampeg_release=0.4 global_volume=11.5'],
        ['spic', 27, 'Spiccato', 72, 'ampeg_release=0.15 global_volume=13'],
      ];
      const files = [];
      let o = header('Solo Violin', 'Solo violin from VSCO 2 Community Edition by Versilian Studios', VSCO, [
        'Mapping by Synkopa: four articulations on keyswitches C0..D#0 (keys 24..27), two',
        'dynamics split at velocity 80, round robins where they were recorded.',
      ]);
      o += '\n<global> sw_lokey=24 sw_hikey=27 sw_default=24 amp_veltrack=60\n';
      for (const [dir, sw, label, c4, env] of arts) {
        const path = `Strings/Solo Violin/${dir}`;
        const parsed = tree
          .filter(p => p.startsWith(path + '/') && p.endsWith('.wav'))
          .map(p => {
            const n = p.slice(path.length + 1, -4);
            const m = /_([A-G]#?\d)_(p|f|v1|v2)(?:_(?:rr|RR)(\d+))?$/.exec(n);
            if (!m) throw new Error(`${path}: ${n}`);
            return { n, path: p, k: key(m[1], c4), loud: m[2] === 'f' || m[2] === 'v2', rr: Number(m[3] ?? 1) };
          });
        const prefix = `${label.toLowerCase()}-`;
        o += `\n<master> sw_last=${sw} sw_label=${label} ${env}\n`;
        const sp = spans(parsed.map(p => p.k), 55, 103);
        for (const loud of [false, true]) {
          o += `<group> ${loud ? 'lovel=80 hivel=127' : 'lovel=1 hivel=79'}\n`;
          for (const r of sp) {
            let takes = parsed.filter(p => p.k === r.root && p.loud === loud);
            if (!takes.length) takes = parsed.filter(p => p.k === r.root);
            takes.sort((a, b) => a.rr - b.rr);
            takes.forEach((t, j) => {
              if (!files.some(f => f.path === t.path)) files.push({ file: `${prefix}${t.n}.wav`, path: t.path });
              const rr = takes.length > 1 ? ` seq_length=${takes.length} seq_position=${j + 1}` : '';
              o += `<region> sample=${prefix}${t.n}.wav lokey=${r.lo} hikey=${r.hi} pitch_keycenter=${r.root}${rr}\n`;
            });
          }
        }
      }
      return { files, sfz: { 'solo-violin': o } };
    },
  },
  {
    id: 'vibraphone',
    name: 'Vibraphone',
    description: 'VCSL soft-mallet vibraphone for Synkopa Keys: 11 notes x 2 dynamics',
    attribution: 'Vibraphone from the Versilian Community Sample Library (VCSL) by Versilian Studios LLC, released to the public domain under CC0-1.0.',
    source: VCSL,
    files: () => {
      const dir = 'Idiophones/Struck Idiophones/Vibraphone/Soft Mallets';
      const names = 'Vibes_soft_A2_v1_rr1_Main Vibes_soft_A2_v2_rr1_Main Vibes_soft_A4_v1_rr1_Main Vibes_soft_A4_v2_rr1_Main Vibes_soft_B3_v1_rr1_Main Vibes_soft_B3_v2_rr1_Main Vibes_soft_C3_v1_rr2_Main Vibes_soft_C3_v2_rr1_Main Vibes_soft_C5_v1_rr1_Main Vibes_soft_C5_v2_rr1_Main Vibes_soft_D4_v1_rr1_Main Vibes_soft_D4_v2_rr1_Main Vibes_soft_E3_v1_rr2_Main Vibes_soft_E3_v2_rr1_Main Vibes_soft_E5_v1_rr1_Main Vibes_soft_E5_v2_rr1_Main Vibes_soft_F2_v1_rr1_Main Vibes_soft_F2_v2_rr1_Main Vibes_soft_F4_v1_rr1_Main Vibes_soft_F4_v2_rr1_Main Vibes_soft_G3_v1_rr1_Main Vibes_soft_G3_v2_rr1_Main'.split(' ');
      // VCSL names its notes with C3 = middle C (C4 is key 72 in their names)
      const parsed = names.map(n => {
        const [, note, v] = /^Vibes_soft_([A-G]\d)_v(\d)_/.exec(n);
        return { n, k: key(note, 72), v: Number(v) };
      });
      const files = names.map(n => ({ file: `${n}.wav`, path: `${dir}/${n}.wav` }));
      let o = header('Vibraphone', 'Vibraphone from the Versilian Community Sample Library by Versilian Studios', VCSL, ['Mapping by Synkopa.']);
      o += '\n<global> ampeg_release=1.8 amp_veltrack=60 global_volume=9.5\n';
      const sp = spans(parsed.map(p => p.k), 48, 91);
      for (const [v, vel] of [[1, 'lovel=1 hivel=80'], [2, 'lovel=81 hivel=127']]) {
        o += `\n<group> ${vel}\n`;
        for (const r of sp) o += `<region> sample=${parsed.find(p => p.k === r.root && p.v === v).n}.wav lokey=${r.lo} hikey=${r.hi} pitch_keycenter=${r.root}\n`;
      }
      return { files, sfz: { vibraphone: o } };
    },
  },
];

// ------------------------------------------------------------------ download and record

const sha = b => createHash('sha256').update(b).digest('hex');
async function fetchCached(url, dst) {
  try {
    await stat(dst);
    return await readFile(dst);
  } catch { /* not cached */ }
  for (let attempt = 1; ; attempt++) {
    try {
      const res = await fetch(url);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const buf = Buffer.from(await res.arrayBuffer());
      await writeFile(dst + '.part', buf);
      await rename(dst + '.part', dst);
      return buf;
    } catch (e) {
      if (attempt === 4) throw new Error(`${url}: ${e.message}`);
      await new Promise(r => setTimeout(r, 2000 * attempt));
    }
  }
}

const only = process.argv.slice(2);
const manifestPath = join(root, 'assets-manifest.json');
const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
for (const kit of KITS.filter(k => !only.length || only.includes(k.id))) {
  const { files, sfz = {} } = await kit.files();
  const cache = join(root, 'target', 'keys-kits', kit.id);
  await mkdir(cache, { recursive: true });
  const samples = [];
  const queue = [...files];
  await Promise.all(Array.from({ length: 6 }, async () => {
    for (let f; (f = queue.shift()); ) {
      const url = raw(kit.source.repo, kit.source.commit, f.path);
      const buf = await fetchCached(url, join(cache, f.file));
      samples.push({ file: f.file, url, sha256: sha(buf), bytes: buf.length });
    }
  }));
  samples.sort((a, b) => files.findIndex(f => f.file === a.file) - files.findIndex(f => f.file === b.file));
  const entry = {
    id: kit.id,
    name: kit.name,
    description: kit.description,
    attribution: kit.attribution,
    source: {
      repo: `https://github.com/${kit.source.repo}`,
      commit: kit.source.commit,
      license: kit.source.license,
      license_url: kit.source.license_url,
      author: kit.source.author,
    },
    samples,
  };
  const i = manifest.instruments.findIndex(k => k.id === kit.id);
  if (i >= 0) manifest.instruments[i] = entry;
  else manifest.instruments.push(entry);
  for (const [id, text] of Object.entries(sfz)) await writeFile(join(instDir, `${id}.sfz`), text);
  const mb = samples.reduce((s, x) => s + x.bytes, 0) / 1e6;
  console.log(`${kit.id}: ${samples.length} files, ${mb.toFixed(1)} MB${Object.keys(sfz).length ? `, wrote ${Object.keys(sfz).join(', ')}.sfz` : ''}`);
}
await writeFile(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
