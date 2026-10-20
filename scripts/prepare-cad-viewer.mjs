import { cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const integration = resolve(root, 'integrations/exav');
const pin = JSON.parse(readFileSync(resolve(integration, 'upstream.json'), 'utf8'));
const patch = readFileSync(resolve(integration, 'r2004.patch'));
const cache = resolve(root, '.cache/cad-viewer');
const output = resolve(cache, 'wasm');
const stamp = resolve(output, 'upstream.json');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const fingerprint = hash(Buffer.concat([
  Buffer.from(JSON.stringify(pin)), patch, readFileSync(fileURLToPath(import.meta.url)),
]));
const run = (bin, args, cwd = root) => execFileSync(bin, args, {
  cwd, stdio: 'inherit', env: { ...process.env, CARGO_TARGET_DIR: resolve(cache, 'target') },
});
const source = resolve(cache, 'source');
const unpack = resolve(source, `exav-unpack-${pin.version}`);
const viewer = resolve(source, 'viewer');
const test = () => run('cargo', ['test', '--manifest-path', resolve(unpack, 'Cargo.toml'),
  '--lib', '--no-default-features', '--features', 'dwg', 'formats::dwg::r2004::tests']);

// Değişmeyen motor her derlemede tekrar derlenmez; üretilen dosyalar da doğrulanır.
if (existsSync(stamp)) {
  const previous = JSON.parse(readFileSync(stamp, 'utf8'));
  if (previous.fingerprint === fingerprint && Object.entries(previous.files).every(([name, digest]) =>
      existsSync(resolve(output, name)) && hash(readFileSync(resolve(output, name))) === digest)) {
    if (process.argv.includes('--test')) test();
    process.exit(0);
  }
}

const download = async (url, destination, digest) => {
  if (existsSync(destination) && hash(readFileSync(destination)) === digest) return;
  const response = await fetch(url);
  if (!response.ok) throw new Error(`CAD source download failed: ${response.status} ${url}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  if (hash(bytes) !== digest) throw new Error(`CAD source checksum mismatch: ${url}`);
  mkdirSync(dirname(destination), { recursive: true });
  writeFileSync(destination, bytes);
};

mkdirSync(source, { recursive: true });
const archive = resolve(cache, `exav-unpack-${pin.version}.crate`);
await download(`https://static.crates.io/crates/exav-unpack/exav-unpack-${pin.version}.crate`,
  archive, pin.unpackSha256);
// Yama her zaman sabit sürümün özgün kaynaklarına uygulanır; Cargo önbelleği değiştirilmez.
run('tar', ['-xzf', archive, '-C', source]);
execFileSync('git', ['apply', '--whitespace=error', '-'], {
  cwd: unpack, input: patch.toString('utf8').replace(/\r\n/g, '\n'), stdio: ['pipe', 'inherit', 'inherit'],
});
mkdirSync(resolve(viewer, 'src'), { recursive: true });
await download(`https://raw.githubusercontent.com/sylvinus/exav/${pin.commit}/crates/exav-viewer/src/lib.rs`,
  resolve(viewer, 'src/lib.rs'), pin.viewerSourceSha256);
writeFileSync(resolve(viewer, 'Cargo.toml'), `[package]
name = "exav-viewer"
version = "${pin.version}"
edition = "2021"
license = "MIT"
[lib]
crate-type = ["cdylib"]
[features]
default = []
dwg = ["exav-render/dwg"]
image = []
model = []
pdf-jpx = []
pdf-jbig2 = []
[dependencies]
exav-render = { version = "=${pin.version}", default-features = false }
wasm-bindgen = "=${pin.bindgenVersion}"
[patch.crates-io]
exav-unpack = { path = "../exav-unpack-${pin.version}" }
[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
`);

let bindgen = 'wasm-bindgen';
try {
  if (execFileSync(bindgen, ['--version'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim()
      !== `wasm-bindgen ${pin.bindgenVersion}`) throw new Error('Version mismatch');
} catch {
  if (process.platform !== 'win32' || process.arch !== 'x64') {
    throw new Error(`Install wasm-bindgen-cli ${pin.bindgenVersion} before building the CAD engine.`);
  }
  const tools = resolve(cache, 'tools');
  const name = `wasm-bindgen-${pin.bindgenVersion}-x86_64-pc-windows-msvc`;
  const toolArchive = resolve(tools, 'wasm-bindgen.tar.gz');
  await download(`https://github.com/wasm-bindgen/wasm-bindgen/releases/download/${pin.bindgenVersion}/${name}.tar.gz`,
    toolArchive, pin.windowsBindgenSha256);
  run('tar', ['-xzf', toolArchive, '-C', tools]);
  bindgen = resolve(tools, name, 'wasm-bindgen.exe');
}
test();
run('cargo', ['rustc', '--manifest-path', resolve(viewer, 'Cargo.toml'), '--release',
  '--target', 'wasm32-unknown-unknown', '--no-default-features', '--features', 'dwg',
  '--crate-type', 'cdylib', '--', '-C', 'link-arg=--max-memory=2147483648']);
run(bindgen, [resolve(cache, 'target/wasm32-unknown-unknown/release/exav_viewer.wasm'),
  '--out-dir', output, '--out-name', 'exav_viewer_dwg', '--target', 'web']);
writeFileSync(resolve(output, 'package.json'), '{"type":"module"}\n');
// Özgün lisans ve attribution, uygulamanın mevcut exav-viewer lisanslarıyla dağıtılır.
cpSync(resolve(root, 'apps/desktop/public/exav-viewer/LICENSE'), resolve(output, 'LICENSE'));
const files = Object.fromEntries(['exav_viewer_dwg.js', 'exav_viewer_dwg_bg.wasm'].map(name =>
  [name, hash(readFileSync(resolve(output, name)))]));
writeFileSync(stamp, JSON.stringify({ ...pin, fingerprint, files }, null, 2));
console.log('Patched CAD engine is ready.');
