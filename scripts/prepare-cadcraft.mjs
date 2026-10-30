import { existsSync, mkdirSync, readFileSync, writeFileSync, cpSync, readdirSync } from 'node:fs';
import { resolve, dirname, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const pin = JSON.parse(readFileSync(resolve(root, 'integrations/cadcraft/upstream.json'), 'utf8'));
const cache = resolve(root, '.cache/cadcraft');
const source = resolve(cache, pin.commit);
const output = resolve(root, 'apps/desktop/public/cadcraft');
const stamp = resolve(output, 'upstream.json');
const run = (bin, args, cwd = root) => execFileSync(bin, args, { cwd, stdio: 'inherit', env: {
  ...process.env, NO_COLOR: 'true', CARGO_TARGET_DIR: resolve(cache, 'target'),
} });
const writeChanged = (path, content) => {
  mkdirSync(dirname(path), { recursive: true });
  if (!existsSync(path) || readFileSync(path, 'utf8') !== content) writeFileSync(path, content);
};
const fingerprint = createHash('sha256').update(JSON.stringify(pin)).update(readFileSync(fileURLToPath(import.meta.url))).digest('hex');
const fileHashes = directory => {
  const files = {};
  if (!existsSync(directory)) return files;
  const visit = path => {
    for (const entry of readdirSync(path, { withFileTypes: true })) {
      const child = resolve(path, entry.name);
      if (entry.isDirectory()) visit(child);
      else if (entry.name !== 'upstream.json') files[relative(directory, child)] = createHash('sha256').update(readFileSync(child)).digest('hex');
    }
  };
  visit(directory);
  return files;
};

if (!existsSync(resolve(source, '.git'))) {
  mkdirSync(cache, { recursive: true });
  run('git', ['clone', '--no-checkout', '--depth', '1', pin.repository, source]);
}
try { execFileSync('git', ['cat-file', '-e', `${pin.commit}^{commit}`], { cwd: source, stdio: 'ignore' }); }
catch { run('git', ['fetch', '--depth', '1', 'origin', pin.commit], source); }
const head = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: source, encoding: 'utf8' }).trim();
if (head !== pin.commit || !existsSync(resolve(source, 'apps/cadcraft-web/src/main.rs'))) {
  run('git', ['checkout', '--force', '--detach', pin.commit], source);
}

const webDir = resolve(source, 'apps/cadcraft-web');
const mainFile = resolve(webDir, 'src/main.rs');
const main = readFileSync(mainFile, 'utf8');
const prefsLine = '                        // `?cpu`: no render state for the app, so the canvas draws with egui shapes.';
if (!main.includes(prefsLine)) throw new Error('CADCraft language adapter no longer matches upstream.');
if (!main.includes('query_param("lang")')) {
  writeChanged(mainFile, main.replace(prefsLine, `                        if let Some(language) = query_param("lang").filter(|language| matches!(language.as_str(), "en" | "tr")) {\n                            app.load_prefs(&format!(r#"{{"interfaceLanguage":"{language}"}}"#));\n                        }\n${prefsLine}`));
}

if (!process.argv.includes('--force') && existsSync(stamp)) {
  const previous = JSON.parse(readFileSync(stamp, 'utf8'));
  const current = fileHashes(output);
  const matches = previous.files && Object.keys(previous.files).length === Object.keys(current).length &&
    Object.entries(current).every(([name, digest]) => previous.files[name] === digest);
  if (previous.fingerprint === fingerprint && matches && existsSync(resolve(output, 'index.html'))) process.exit(0);
}

run(process.env.TRUNK || 'trunk', ['build', '--release', '--public-url', './', '--dist', output], webDir);
const notices = ['LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'ATTRIBUTION.md'];
for (const name of notices) {
  const file = resolve(source, name);
  if (existsSync(file)) cpSync(file, resolve(output, name));
}
writeFileSync(stamp, JSON.stringify({ ...pin, fingerprint, files: fileHashes(output) }, null, 2));
console.log('CADCraft web editor is ready.');
