// Builds Synkopa Demo Synth (crates/demo-synth) and lays it out as a VST3 bundle, by default in
// app/resources/plugins, which the installer ships and Synkopa scans (see docs/architecture.md,
// "Plugin instruments"). The release and CI workflows run it before building the app; the
// host's end-to-end instrument test points WAVE_VST3_TEST_INSTRUMENT at the result.
//   node scripts/build-demo-synth.mjs [--debug] [--out DIR]
import { spawnSync } from 'node:child_process';
import { copyFile, mkdir, rm } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const debug = args.includes('--debug');
const at = args.indexOf('--out');
const out = at >= 0 && args[at + 1] ? resolve(args[at + 1]) : join(root, 'app', 'resources', 'plugins');

// The bundle keeps its file name from before the rename: projects store the module's path.
const name = 'WAVE Demo Synth';
const layout = {
  win32: { arch: 'x86_64-win', built: 'wave_demo_synth.dll', ext: '.vst3' },
  linux: { arch: 'x86_64-linux', built: 'libwave_demo_synth.so', ext: '.so' },
}[process.platform];
if (!layout) throw new Error(`Synkopa hosts VST3 on Windows and Linux; nothing to build on ${process.platform}`);

const cargo = spawnSync('cargo', ['build', '--locked', '-p', 'wave-demo-synth', ...(debug ? [] : ['--release'])], { cwd: root, stdio: 'inherit', shell: process.platform === 'win32' });
if (cargo.status !== 0) process.exit(cargo.status ?? 1);

const bundle = join(out, name + '.vst3');
const dir = join(bundle, 'Contents', layout.arch);
await rm(bundle, { recursive: true, force: true });
await mkdir(dir, { recursive: true });
await copyFile(join(root, 'target', debug ? 'debug' : 'release', layout.built), join(dir, name + layout.ext));
console.log(bundle);
