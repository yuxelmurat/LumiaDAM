const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');
const ts = require('typescript');

function setup(search = '') {
  const messages = [], opened = [];
  let receive, repaints = 0;
  const parent = { postMessage: (data) => messages.push(data) };
  const window = { addEventListener: (_, fn) => { receive = fn; }, lumiadamOpen: (...args) => opened.push(args), lumiadamRepaint: () => repaints++ };
  runInNewContext(readFileSync(join(__dirname, '../../..', 'integrations/photocraft/bridge.js'), 'utf8'), {
    window, parent, location: { origin: 'http://tauri.localhost', search }, URLSearchParams, ArrayBuffer, Uint8Array,
    crypto: { randomUUID: () => 'export-id' },
  });
  const send = (data, origin = 'http://tauri.localhost', source = parent) => receive({ source, origin, data: { channel: 'lumiadam-editor-v1', ...data } });
  window.lumiadamReady();
  return { window, messages, opened, send, get repaints() { return repaints; } };
}

test('Editör yabancı kökenden ve farklı pencereden dosya kabul etmez', () => {
  const s = setup();
  const open = { type: 'open', name: '9.jpg', bytes: new ArrayBuffer(1) };
  s.send(open, 'https://example.com');
  s.send(open, undefined, {});
  assert.equal(s.opened.length, 0);
  s.send(open);
  s.send(open);
  assert.equal(s.opened.length, 1);
  assert.equal(s.opened[0][0], '9.jpg');
});

test('Kayıt onayı gelmeden ikinci export gönderilmez, yanlış onay kilidi açmaz', () => {
  const s = setup();
  s.send({ type: 'open', name: '9.jpg', bytes: new ArrayBuffer(1) });
  const bytes = new Uint8Array([1, 2, 3]);
  s.window.lumiadamExport('9.png', bytes);
  bytes[0] = 8;
  assert.equal(new Uint8Array(s.messages.at(-1).bytes)[0], 1);
  assert.throws(() => s.window.lumiadamExport('9.png', bytes), /pending/);
  s.send({ type: 'saved', id: 'wrong' });
  assert.throws(() => s.window.lumiadamExport('9.png', bytes), /pending/);
  s.send({ type: 'saved', id: 'export-id' });
  assert.doesNotThrow(() => s.window.lumiadamExport('9.png', bytes));
});

test('Geçersiz çıktı türü ve boş çıktı ana uygulamaya gönderilmez', () => {
  const s = setup();
  assert.throws(() => s.window.lumiadamExport('9.jpg', new Uint8Array([1])), /Open a library/);
  s.send({ type: 'open', name: '9.jpg', bytes: new ArrayBuffer(1) });
  assert.throws(() => s.window.lumiadamExport('9.exe', new Uint8Array([1])), /Export as/);
  assert.throws(() => s.window.lumiadamExport('9.jpg', new Uint8Array()), /256 MB/);
  assert.equal(s.messages.filter((m) => m.type === 'export').length, 0);
});

test('Fotoğraf editörü dil değiştirirken açık kaynağı ve bekleyen kaydı korur', () => {
  const s = setup('?lang=tr');
  assert.equal(s.window.lumiadamLanguage(), 'tr');
  assert.throws(() => s.window.lumiadamExport('Save.png', new Uint8Array([1])), /Önce kütüphaneden/);
  s.send({ type: 'open', name: 'Save.jpg', bytes: new ArrayBuffer(1) });
  s.window.lumiadamExport('Save.png', new Uint8Array([1]));
  s.send({ type: 'language', language: 'en' });
  assert.equal(s.window.lumiadamLanguage(), 'en');
  assert.equal(s.repaints, 1);
  assert.equal(s.opened.length, 1);
  assert.equal(s.opened[0][0], 'Save.jpg');
  assert.throws(() => s.window.lumiadamExport('Save.png', new Uint8Array([1])), /pending/);
  s.send({ type: 'language', language: 'tr' });
  s.send({ type: 'open', name: 'another.jpg', bytes: new ArrayBuffer(1) });
  assert.equal(s.opened.length, 1);
  s.send({ type: 'saved', id: 'export-id' });
  assert.doesNotThrow(() => s.window.lumiadamExport('Save.png', new Uint8Array([1])));
});

test('Fotoğraf editörü yetkisiz ve desteklenmeyen dil değişimini reddeder', () => {
  const s = setup('?lang=unknown'), change = { type: 'language', language: 'tr' };
  s.send(change, 'https://example.com'); s.send(change, undefined, {}); s.send({ ...change, channel: 'other' });
  s.send({ ...change, language: 'de' }); s.send({ ...change, language: { value: 'tr' } });
  assert.equal(s.window.lumiadamLanguage(), 'en');
  assert.equal(s.repaints, 0);
  s.send(change); s.window.lumiadamNotice();
  assert.equal(s.window.lumiadamLanguage(), 'tr');
  assert.match(s.messages.at(-1).message, /Kütüphane/);
});

test('CADCraft giriş adresi üst pencere köprüsüyle açılır ve DWG/DXF ile sınırlıdır', () => {
  const registry = {};
  runInNewContext(ts.transpileModule(readFileSync(join(__dirname, '../src/components/Editor/editors.ts'), 'utf8'),
    { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, { exports: registry });
  assert.equal(registry.editors.cadcraft.path, '/cadcraft/index.html?host=parent');
  assert.deepEqual(Array.from(registry.editors.cadcraft.inputs), ['dwg', 'dxf']);
  assert.match(registry.editors.cadcraft.hint, /experimental/i);
});
