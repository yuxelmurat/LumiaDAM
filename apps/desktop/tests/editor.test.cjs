const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');

function setup() {
  const messages = [], opened = [];
  let receive;
  const parent = { postMessage: (data) => messages.push(data) };
  const window = { addEventListener: (_, fn) => { receive = fn; }, lumiadamOpen: (...args) => opened.push(args) };
  runInNewContext(readFileSync(join(__dirname, '../../..', 'integrations/photocraft/bridge.js'), 'utf8'), {
    window, parent, location: { origin: 'http://tauri.localhost' }, ArrayBuffer, Uint8Array,
    crypto: { randomUUID: () => 'export-id' },
  });
  const send = (data, origin = 'http://tauri.localhost', source = parent) => receive({ source, origin, data: { channel: 'lumiadam-editor-v1', ...data } });
  window.lumiadamReady();
  return { window, messages, opened, send };
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
