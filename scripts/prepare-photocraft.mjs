import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const integration = resolve(root, 'integrations/photocraft');
const pin = JSON.parse(readFileSync(resolve(integration, 'upstream.json'), 'utf8'));
const source = resolve(process.env.PHOTOCRAFT_SOURCE || resolve(root, '.cache/photocraft', pin.commit));
const output = resolve(root, 'apps/desktop/public/photocraft');
const stamp = resolve(output, 'upstream.json');
const fingerprint = createHash('sha256').update(JSON.stringify(pin))
  .update(readFileSync(resolve(integration, 'host.rs'))).update(readFileSync(resolve(integration, 'bridge.js')))
  .update(readFileSync(fileURLToPath(import.meta.url))).digest('hex');
const manifest = JSON.stringify({ ...pin, fingerprint });
const writeChanged = (path, text) => {
  if (!existsSync(path) || readFileSync(path, 'utf8') !== text) writeFileSync(path, text);
};
const run = (bin, args, cwd = root) => execFileSync(bin, args, { cwd, stdio: 'inherit', env: { ...process.env, NO_COLOR: 'true' } });
const replace = (text, before, after) => {
  if (!text.includes(before)) throw new Error(`PhotoCraft adapter no longer matches upstream: ${before}`);
  return text.replace(before, after);
};
const replaceSection = (text, start, end, replacement) => {
  const from = text.indexOf(start), to = text.indexOf(end, from + start.length);
  if (from < 0 || to < 0) throw new Error(`PhotoCraft adapter section changed: ${start}`);
  return text.slice(0, from) + replacement + text.slice(to);
};
if (!process.argv.includes('--force') && existsSync(stamp) && existsSync(resolve(output, 'index.html')) && readFileSync(stamp, 'utf8') === manifest) process.exit(0);
if (!existsSync(resolve(source, '.git'))) {
  mkdirSync(dirname(source), { recursive: true });
  run('git', ['clone', '--no-checkout', '--depth', '1', pin.repository, source]);
}
try { execFileSync('git', ['cat-file', '-e', `${pin.commit}^{commit}`], { cwd: source, stdio: 'ignore' }); }
catch { run('git', ['fetch', '--depth', '1', 'origin', pin.commit], source); }
// Çalışma dosyaları yerine sabit commit'in içeriğini okur; mevcut değişiklikleri silmez.
const upstream = (path) => execFileSync('git', ['show', `${pin.commit}:${path}`], { cwd: source, encoding: 'utf8' });
const head = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: source, encoding: 'utf8' }).trim();
if (head !== pin.commit) run('git', ['checkout', '--detach', pin.commit], source);
else if (!existsSync(resolve(source, 'Cargo.toml'))) run('git', ['checkout', '--detach', pin.commit], source);
let web = upstream('apps/photocraft-web/src/web.rs');
web = replace(web, 'type Inbox =', '#[path = "lumiadam_host.rs"]\nmod lumiadam_host;\n\ntype Inbox =');
web = replace(web, 'let served: Served = Arc::default();', 'lumiadam_host::install(inbox.clone(), cc.egui_ctx.clone());\n                    let served: Served = Arc::default();');
web = replace(web, 'download(path, bytes)', 'lumiadam_host::write(path, bytes)');
web = replace(web, 'let mut app = PhotocraftApp::new(session, services(inbox.clone()));', 'let mut app = PhotocraftApp::new(session, services(inbox.clone()));\n                    app.session.edit_prefs(|prefs| prefs.interface.language = "en".into());');
web = replaceSection(web, '/// Everything File', 'const CANVAS_ID', '');
web = replaceSection(web, '        let dropped = ctx.input_mut', '        self.serve_fonts(ctx);',
  '        if ctx.input_mut(|i| !std::mem::take(&mut i.raw.dropped_files).is_empty()) { lumiadam_host::open_notice(); }\n');
web = replaceSection(web, '            // The browser\'s file picker', '            // No save dialog',
  '            FileDialogRequest::Open { .. } => { lumiadam_host::open_notice(); reply.send(None); },\n');
web = replaceSection(web, '/// Trigger a browser download', '/// Rust bindings to the browser', '');
writeChanged(resolve(source, 'apps/photocraft-web/src/web.rs'), web);
writeChanged(resolve(source, 'apps/photocraft-web/src/lumiadam_host.rs'), readFileSync(resolve(integration, 'host.rs'), 'utf8'));
let html = upstream('apps/photocraft-web/index.html');
html = html.replace(' data-cargo-features="heif"', '').replace(' data-wasm-opt="z"', '');
html = replace(html, '</head>', '<script src="bridge.js"></script>\n<link data-trunk rel="copy-file" href="bridge.js">\n</head>');
writeChanged(resolve(source, 'apps/photocraft-web/index.html'), html);
writeChanged(resolve(source, 'apps/photocraft-web/bridge.js'), readFileSync(resolve(integration, 'bridge.js'), 'utf8'));
if (process.argv.includes('--source-only')) process.exit(0);
run(process.env.TRUNK || 'trunk', ['build', '--release', '--dist', output], resolve(source, 'apps/photocraft-web'));
for (const name of ['LICENSE-MIT', 'LICENSE-APACHE', 'ATTRIBUTION.md']) writeFileSync(resolve(output, name), upstream(name));
// Gömülü ikonlar, sözlük ve renk paletleri kendi lisans bildirimlerini de taşır.
const notices = execFileSync('git', ['ls-tree', '-r', '--name-only', pin.commit], { cwd: source, encoding: 'utf8' }).split('\n')
  .filter((path) => /(^|\/)(LICENSE[^/]*|OFL[^/]*|NOTICE[^/]*)$/i.test(path) && !path.startsWith('docs/brand/'));
for (const path of notices) {
  const target = resolve(output, 'notices', path);
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, upstream(path));
}
writeFileSync(stamp, manifest);
