import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const common = resolve(root, 'integrations/craft-editors');
const run = (bin, args, cwd = root) => execFileSync(bin, args, { cwd, stdio: 'inherit',
  env: { ...process.env, NO_COLOR: 'true', CARGO_TARGET_DIR: resolve(root, '.cache/craft-target') } });
const writeChanged = (path, text) => {
  mkdirSync(dirname(path), { recursive: true });
  if (!existsSync(path) || readFileSync(path, 'utf8') !== text) writeFileSync(path, text);
};
const replace = (text, before, after) => {
  if (!text.includes(before)) throw new Error(`Craft editor adapter no longer matches upstream: ${before}`);
  return text.replace(before, after);
};
const section = (text, start, end, replacement) => {
  const from = text.indexOf(start), to = text.indexOf(end, from + start.length);
  if (from < 0 || to < 0) throw new Error(`Craft editor section changed: ${start}`);
  return text.slice(0, from) + replacement + text.slice(to);
};
const configs = {
  pdfcraft: { channel: 'lumiadam-pdf-v1', inputs: ['pdf'], outputs: ['pdf'], version: '0.7.1' },
  vectorcraft: { channel: 'lumiadam-vector-v1', inputs: ["svg","svgz","pdf","ai","vectorcraft","drawcraft","jpg","jpeg","png","webp","bmp","tif","tiff","gif"],
    outputs: ["svg","svgz","pdf","vectorcraft","jpg","jpeg","png","webp"], version: '0.10.0' },
};
if (!process.argv.includes('--source-only') && !process.argv.some(arg => Object.hasOwn(configs, arg))) {
  execFileSync(process.execPath, [resolve(root, 'scripts/prepare-photocraft.mjs')], { cwd: root, stdio: 'inherit' });
}
for (const [name, config] of Object.entries(configs)) {
  if (process.argv.some(arg => Object.hasOwn(configs, arg)) && !process.argv.includes(name)) continue;
  const integration = resolve(root, 'integrations', name);
  const pin = JSON.parse(readFileSync(resolve(integration, 'upstream.json'), 'utf8'));
  const output = resolve(root, 'apps/desktop/public', name), stamp = resolve(output, 'upstream.json');
  const fingerprint = createHash('sha256').update(JSON.stringify({pin,config}))
    .update(readFileSync(resolve(common, 'host.rs'))).update(readFileSync(resolve(common, 'bridge.js')))
    .update(readFileSync(fileURLToPath(import.meta.url)))
    .update(name === 'pdfcraft' ? readFileSync(resolve(integration, 'main.rs')) : '').digest('hex');
  const manifest = JSON.stringify({ ...pin, ...config, fingerprint });
  if (!process.argv.includes('--force') && existsSync(stamp) && existsSync(resolve(output, 'index.html')) &&
      readFileSync(stamp, 'utf8') === manifest) continue;
  const source = resolve(root, '.cache', name, 'source');
  if (!existsSync(resolve(source, '.git'))) {
    mkdirSync(dirname(source), {recursive:true});
    run('git', ['clone','--no-checkout','--depth','1',pin.repository,source]);
  }
  try { execFileSync('git',['cat-file','-e',`${pin.commit}^{commit}`], {cwd:source,stdio:'ignore'}); }
  catch { run('git',['fetch','--depth','1','origin',pin.commit],source); }
  const head = execFileSync('git',['rev-parse','HEAD'],{cwd:source,encoding:'utf8'}).trim();
  if (head !== pin.commit || !existsSync(resolve(source,'Cargo.toml'))) run('git',['checkout','--detach',pin.commit],source);
  const upstream = path => execFileSync('git',['show',`${pin.commit}:${path}`],{cwd:source,encoding:'utf8'});
  const webDir = `apps/${name}-web`;
  writeChanged(resolve(source,webDir,'src/lumiadam_host.rs'),readFileSync(resolve(common,'host.rs'),'utf8'));
  if (name === 'pdfcraft') {
    writeChanged(resolve(source,webDir,'src/main.rs'),readFileSync(resolve(integration,'main.rs'),'utf8'));
    writeChanged(resolve(source,webDir,'Cargo.toml'),upstream(`${webDir}/Cargo.toml`) + '\nwasm-bindgen = "0.2"\n');
    let widgets = upstream('crates/ui-egui/src/widgets.rs');
    // ArtCraft'ın özel lisanslı logo varlıkları değiştirilmiş pakete gömülmez.
    widgets = section(widgets,'/// The ArtCraft wordmark','/// Buttons for every community link',
      'pub fn artcraft_logo(ui: &mut egui::Ui, height: f32) -> Response { ui.label(egui::RichText::new("PdfCraft").size(height)) }\n' +
      'pub fn artcraft_mark(ui: &mut egui::Ui, size: f32) -> Response { ui.allocate_response(egui::vec2(0.0, size), egui::Sense::hover()) }\n\n');
    writeChanged(resolve(source,'crates/ui-egui/src/widgets.rs'),widgets);
    let lib = upstream('crates/ui-egui/src/lib.rs');
    lib = replace(lib,'    pub fn open_dialog(&mut self) {',
      '    pub fn open_dialog(&mut self) {\n        if self.host_save.is_some() { self.notify("Choose another PDF in LumiaDAM Libraries."); return; }');
    writeChanged(resolve(source,'crates/ui-egui/src/lib.rs'),lib);
  } else {
    let web = upstream(`${webDir}/src/web.rs`);
    web = replace(web,'type Inbox =','#[path = "lumiadam_host.rs"]\nmod lumiadam_host;\n\ntype Inbox =');
    web = replace(web,'                let ctx = &cc.egui_ctx;',
      '                let ctx = &cc.egui_ctx;\n                lumiadam_host::install(host.inbox.clone(), ctx.clone());');
    web = replace(web,'                Ok(Box::new(WebShell { app: Some(app), host, canvas: page, loss }))',
      '                let mut app = app;\n                app.session.prefs.interface_language = "en".into();\n                Ok(Box::new(WebShell { app: Some(app), host, canvas: page, loss }))');
    web = section(web,'        let dropped = ctx.input_mut','        app.logic(ctx);',
      '        if ctx.input_mut(|i| !std::mem::take(&mut i.raw.dropped_files).is_empty()) { lumiadam_host::open_notice(); }\n');
    web = section(web,'        open_async: Some(Box::new(move || {','        download: Some',
      '        open_async: Some(Box::new(move || { lumiadam_host::open_notice(); })),\n');
    web = replace(web,'download(name, bytes)','lumiadam_host::write(name, bytes)');
    web = replace(web,'    let open_inbox = inbox.clone();','');
    web = replace(web,'{DropTarget, PlaceArrival, PlaceInbox}','{PlaceArrival, PlaceInbox}');
    web = section(web,'/// Trigger a browser download','/// An object URL of a blob','');
    writeChanged(resolve(source,webDir,'src/web.rs'),web);
  }
  let html = upstream(`${webDir}/index.html`).replace(' data-wasm-opt="z"','');
  html = replace(html,'</head>',`<script>window.LUMIADAM_EDITOR=${JSON.stringify(config)};</script>\n<script src="bridge.js"></script>\n<link data-trunk rel="copy-file" href="bridge.js">\n</head>`);
  writeChanged(resolve(source,webDir,'index.html'),html);
  writeChanged(resolve(source,webDir,'bridge.js'),readFileSync(resolve(common,'bridge.js'),'utf8'));
  if (process.argv.includes('--source-only')) continue;
  run(process.env.TRUNK || 'trunk',['build','--release','--public-url','./','--dist',output],resolve(source,webDir));
  for (const notice of ['LICENSE-MIT','LICENSE-APACHE','NOTICE','ATTRIBUTION.md','ASSETS.md']) {
    if (existsSync(resolve(source,notice))) writeChanged(resolve(output,notice),upstream(notice));
  }
  const notices = execFileSync('git',['ls-tree','-r','--name-only',pin.commit],{cwd:source,encoding:'utf8'}).split('\n')
    .filter(path => /(^|\/)(LICENSE[^/]*|OFL[^/]*|NOTICE[^/]*)$/i.test(path) && !path.startsWith('docs/brand/'));
  for (const path of notices) writeChanged(resolve(output,'notices',path),upstream(path));
  writeChanged(stamp,manifest);
}
