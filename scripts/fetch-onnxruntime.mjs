// Linux: puts Microsoft's ONNX Runtime library into app/resources/ort for the installer.
// Synkopa loads it at run time (`ort` `load-dynamic`) instead of linking the prebuilt `ort`
// downloads, which needs glibc 2.38: this one needs glibc 2.27, so the packages and the
// AppImage start on Ubuntu 22.04 and Debian 12. The tarball is checked against its SHA-256.
//   node scripts/fetch-onnxruntime.mjs
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFile, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

// keep in step with the `api-NN` feature of `ort` in crates/rmvpe/Cargo.toml (ONNX Runtime >= 1.NN)
const VERSION = '1.22.0';
const URL = `https://github.com/microsoft/onnxruntime/releases/download/v${VERSION}/onnxruntime-linux-x64-${VERSION}.tgz`;
const SHA256 = '8344d55f93d5bc5021ce342db50f62079daf39aaafb5d311a451846228be49b3';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const work = join(root, 'target', 'ort-src');
const out = join(root, 'app', 'resources', 'ort');
await mkdir(work, { recursive: true });
await mkdir(out, { recursive: true });

const tgz = join(work, `onnxruntime-${VERSION}.tgz`);
let buf;
try { buf = await readFile(tgz); } catch { buf = null; }
const sha = b => createHash('sha256').update(b).digest('hex');
if (!buf || sha(buf) !== SHA256) {
  const res = await fetch(URL);
  if (!res.ok) throw new Error(`${URL}: HTTP ${res.status}`);
  buf = Buffer.from(await res.arrayBuffer());
  if (sha(buf) !== SHA256) throw new Error(`${URL}: checksum mismatch`);
  await writeFile(tgz, buf);
}
const dir = join(work, `onnxruntime-linux-x64-${VERSION}`);
await rm(dir, { recursive: true, force: true });
const tar = spawnSync('tar', ['xzf', tgz, '-C', work], { stdio: 'inherit' });
if (tar.status !== 0) throw new Error('tar failed');
await copyFile(join(dir, 'lib', `libonnxruntime.so.${VERSION}`), join(out, 'libonnxruntime.so.1'));
await copyFile(join(dir, 'LICENSE'), join(out, 'LICENSE.txt'));
console.log(`ONNX Runtime ${VERSION} -> ${out}`);
