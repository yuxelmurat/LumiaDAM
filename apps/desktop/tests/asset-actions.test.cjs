const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');
const ts = require('typescript');

function setup(openWithDefault = async () => {}) {
  const exports = {};
  const source = readFileSync(join(__dirname, '../src/components/AssetGrid/assetActions.ts'), 'utf8');
  runInNewContext(ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText,
    { exports, require: () => ({ api: { openWithDefault } }) });
  return exports;
}

test('Görseller büyütülür; ZIP/MAX/PDF Windows ilişkilendirmesine gönderilir', async () => {
  const external = [], preview = [], editor = [];
  const { activateAsset } = setup(async path => external.push(path));
  for (const extension of ['JPEG', 'png', 'hdr', 'exr', 'avif', 'tga', 'psd', 'zip', 'max', 'pdf', 'pcraft']) {
    const asset = { id: extension, extension, filePath: `A:/Arşiv & models/9.${extension}` };
    await activateAsset(asset, a => preview.push(a.id), a => editor.push(a.id));
  }
  assert.deepEqual(preview, ['JPEG', 'png', 'hdr', 'exr', 'avif', 'tga', 'psd']);
  assert.deepEqual(external, ['A:/Arşiv & models/9.zip', 'A:/Arşiv & models/9.max', 'A:/Arşiv & models/9.pdf']);
  assert.deepEqual(editor, ['pcraft']);
});

test('PhotoEditor yalnız desteklenen görsel ve proje formatlarında etkin olur', () => {
  const { canEditImage } = setup();
  for (const extension of ['JPEG', 'png', 'webp', 'bmp', 'tif', 'tiff', 'pcraft']) assert.equal(canEditImage({ extension }), true);
  for (const extension of ['zip', 'max', 'pdf', 'hdr', 'exr', 'svg']) assert.equal(canEditImage({ extension }), false);
  assert.equal(canEditImage(null), false);
});

test('Windows açma hatası gizlenmez; arayüzün gösterebilmesi için döner', async () => {
  const { activateAsset } = setup(async () => { throw new Error('No default application'); });
  await assert.rejects(() => activateAsset({ extension: 'max', filePath: 'A:/scene.max' }, () => {}, () => {}), /No default application/);
});
