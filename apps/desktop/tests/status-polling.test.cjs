const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');
const ts = require('typescript');
function setup(read, panelOpen = false) {
  const timers = new Map(), values = [], errors = [];
  let nextId = 0;
  const exports = {};
  const source = readFileSync(join(__dirname, '../src/components/StatusBar/statusPolling.ts'), 'utf8');
  const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  runInNewContext(compiled, { exports, setTimeout: (fn, delay) => { timers.set(++nextId, { fn, delay }); return nextId; }, clearTimeout: (id) => timers.delete(id) });
  const poller = exports.createStatusPolling({ read, onStatus: (v) => values.push(v), onError: (e) => errors.push(e), panelOpen: () => panelOpen });
  return { poller, timers, values, errors };
}

test('Status boşta 30 saniye, iş sırasında veya panel açıkken 2 saniye bekler', async () => {
  for (const [statuses, open, delay] of [[[], false, 30000], [[{ scanning: true }], false, 2000], [[{ previewPending: 1 }], false, 2000], [[], true, 2000]]) {
    const { poller, timers } = setup(async () => statuses, open);
    await poller.refresh();
    assert.equal([...timers.values()][0].delay, delay);
    poller.stop();
    assert.equal(timers.size, 0);
  }
});

test('Eşzamanlı yenilemeler yalnız bir status sorgusu gönderir', async () => {
  let calls = 0, finish;
  const { poller } = setup(() => { calls++; return new Promise(resolve => { finish = resolve; }); });
  const a = poller.refresh(), b = poller.refresh();
  assert.equal(calls, 1);
  finish([]); await Promise.all([a, b]); poller.stop();
});

test('Kaynak beklenirken kalan işler çalışma sayılmaz; diğer kütüphane çalışıyorsa durum sık yenilenir', async () => {
  for (const [statuses, delay] of [
    [[{ previewPending: 80000, previewPaused: true }], 30000],
    [[{ previewPending: 80000, previewPaused: true }, { previewPending: 1, previewPaused: false }], 2000],
  ]) {
    const { poller, timers } = setup(async () => statuses);
    await poller.refresh();
    assert.equal([...timers.values()][0].delay, delay);
    poller.stop();
  }
});

test('Birçok preview olayı tek bir durum yenilemesinde birleştirilir', async () => {
  const { poller, timers } = setup(async () => []);
  await poller.refresh();
  for (let i = 0; i < 100; i++) poller.invalidate();
  assert.equal(timers.size, 1);
  assert.equal([...timers.values()][0].delay, 1000);
  poller.stop();
});

test('Unmount sonrası geç gelen durum sonucu yazılmaz ve yeni timer başlatılmaz', async () => {
  let finish;
  const { poller, timers, values } = setup(() => new Promise(resolve => { finish = resolve; }));
  const pending = poller.refresh(); poller.stop(); finish([]); await pending;
  assert.equal(values.length, 0);
  assert.equal(timers.size, 0);
});
