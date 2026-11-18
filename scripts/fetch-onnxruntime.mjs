// Linux: puts Microsoft's ONNX Runtime library into app/resources/ort for the installer.
// Synkopa loads it at run time (`ort` `load-dynamic`) instead of linking the prebuilt `ort`
// downloads, which needs glibc 2.38: this one needs glibc 2.27, so the packages and the
// AppImage start on Ubuntu 22.04 and Debian 12. It comes from Microsoft's NuGet package
// (Microsoft.ML.OnnxRuntime, runtimes/linux-x64): the GitHub release tarball it used to come from
// is gone (github.com/microsoft/onnxruntime answers 404). Same build, same glibc 2.27 floor. The
// package is checked against its SHA-256.
//   node scripts/fetch-onnxruntime.mjs
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFile, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

// keep in step with the `api-NN` feature of `ort` in crates/rmvpe/Cargo.toml (ONNX Runtime >= 1.NN)
const VERSION = '1.22.0';
const URL = `https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime/${VERSION}/microsoft.ml.onnxruntime.${VERSION}.nupkg`;
const SHA256 = 'd571e63a2329baacb713f441e65ad75284de354db6e1ac435fe4bebbb417986a';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const work = join(root, 'target', 'ort-src');
const out = join(root, 'app', 'resources', 'ort');
await mkdir(work, { recursive: true });
await mkdir(out, { recursive: true });

const pkg = join(work, `onnxruntime-${VERSION}.nupkg`);
let buf;
try { buf = await readFile(pkg); } catch { buf = null; }
const sha = b => createHash('sha256').update(b).digest('hex');
if (!buf || sha(buf) !== SHA256) {
  const res = await fetch(URL);
  if (!res.ok) throw new Error(`${URL}: HTTP ${res.status}`);
  buf = Buffer.from(await res.arrayBuffer());
  if (sha(buf) !== SHA256) throw new Error(`${URL}: checksum mismatch`);
  await writeFile(pkg, buf);
}
// a .nupkg is a zip: take the Linux library and the licence
const dir = join(work, `onnxruntime-linux-x64-${VERSION}`);
await rm(dir, { recursive: true, force: true });
const unzip = spawnSync('unzip', ['-o', '-j', pkg, 'runtimes/linux-x64/native/libonnxruntime.so', 'LICENSE', '-d', dir], { stdio: 'inherit' });
if (unzip.status !== 0) throw new Error('unzip failed');
await copyFile(join(dir, 'libonnxruntime.so'), join(out, 'libonnxruntime.so.1'));
await copyFile(join(dir, 'LICENSE'), join(out, 'LICENSE.txt'));
console.log(`ONNX Runtime ${VERSION} -> ${out}`);
