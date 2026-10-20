const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');
const ts = require('typescript');

function setup(openWithDefault = async () => {}) {
  const exports = {};
  const registry = {};
  runInNewContext(ts.transpileModule(readFileSync(join(__dirname, '../src/components/Editor/editors.ts'), 'utf8'),
    { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, { exports: registry });
  const source = readFileSync(join(__dirname, '../src/components/AssetGrid/assetActions.ts'), 'utf8');
  runInNewContext(ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText,
    { exports, require: path => path.includes('editors') ? registry : ({ api: { openAssetExternal: asset => openWithDefault(asset.filePath) } }) });
  return { ...exports, canOpenInEditor: registry.canOpenInEditor };
}

test('Görseller büyütülür; projeler dahili editöre, ZIP/MAX Windows uygulamasına gider', async () => {
  const external = [], preview = [], editor = [];
  const { activateAsset } = setup(async path => external.push(path));
  for (const extension of ['JPEG', 'png', 'hdr', 'exr', 'avif', 'tga', 'psd', 'zip', 'max', 'pdf', 'pcraft', 'vectorcraft', 'drawcraft', 'svgz']) {
    const asset = { id: extension, extension, filePath: `A:/Arşiv & models/9.${extension}` };
    await activateAsset(asset, a => preview.push(a.id), (a, kind) => editor.push([a.id, kind]));
  }
  assert.deepEqual(preview, ['JPEG', 'png', 'hdr', 'exr', 'avif', 'tga', 'psd']);
  assert.deepEqual(external, ['A:/Arşiv & models/9.zip', 'A:/Arşiv & models/9.max']);
  assert.deepEqual(editor, [['pdf','pdf'],['pcraft','photo'],['vectorcraft','vector'],['drawcraft','vector'],['svgz','vector']]);
});

test('PhotoEditor yalnız desteklenen görsel ve proje formatlarında etkin olur', () => {
  const { canOpenInEditor } = setup();
  for (const extension of ['JPEG', 'png', 'webp', 'bmp', 'tif', 'tiff', 'pcraft']) assert.equal(canOpenInEditor({ extension }, 'photo'), true);
  for (const extension of ['zip', 'max', 'pdf', 'hdr', 'exr', 'svg']) assert.equal(canOpenInEditor({ extension }, 'photo'), false);
  assert.equal(canOpenInEditor(null, 'photo'), false);
});

test('DWG ve DXF çift tıklaması mevcut CAD Viewer içinde kalır', async () => {
  const external = [], cad = [];
  const { activateAsset } = setup(async path => external.push(path));
  for (const extension of ['dwg', 'DXF']) {
    const asset = { id: extension, extension, filePath: `A:/drawings/plan.${extension}` };
    await activateAsset(asset, () => {}, () => {}, value => cad.push(value.id));
  }
  assert.deepEqual(cad, ['dwg', 'DXF']);
  assert.deepEqual(external, []);
});

test('CADCraft yalnız DWG/DXF çizimlerinde etkinleşir; diğer editörlere aktarılmaz', () => {
  const { canOpenInEditor } = setup();
  for (const extension of ['dwg', 'DXF']) assert.equal(canOpenInEditor({ extension }, 'cadcraft'), true);
  for (const extension of ['jpg', 'pdf', 'zip', 'max', 'exe']) assert.equal(canOpenInEditor({ extension }, 'cadcraft'), false);
  assert.equal(canOpenInEditor(null, 'cadcraft'), false);
});

test('Windows açma hatası gizlenmez; arayüzün gösterebilmesi için döner', async () => {
  const { activateAsset } = setup(async () => { throw new Error('No default application'); });
  await assert.rejects(() => activateAsset({ extension: 'max', filePath: 'A:/scene.max' }, () => {}, () => {}, () => {}), /No default application/);
});

test('Max merge eylemi MAX, ZIP ve RAR dosyalarında kullanılabilir', () => {
  const { canMergeInMax } = setup();
  assert.equal(typeof canMergeInMax, 'function', 'Max merge eylemi henüz yok');
  for (const extension of ['max', 'MAX', 'zip', 'ZIP', 'rar', 'RAR']) assert.equal(canMergeInMax({ extension }), true);
  for (const extension of ['jpg', 'pdf', 'dwg', 'fbx', 'exe']) assert.equal(canMergeInMax({ extension }), false);
  assert.equal(canMergeInMax(null), false);
});
