const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');
const ts = require('typescript');

// Gerçek store'u çalıştırır; yalnız Tauri çağrılarını ve saati denetim altına alır.
const source = readFileSync(join(__dirname, '../src/store/index.ts'), 'utf8');
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;

function setup() {
  const requests = [];
  const timers = [];
  const errors = [];
  const exports = {};
  const api = {
    searchAssets: (payload) => new Promise((resolve, reject) => {
      requests.push({ payload, resolve, reject });
    }),
  };
  runInNewContext(compiled, {
    exports,
    require: (name) => name === '../api' ? { api } : require(name),
    setTimeout: (resolve, delay) => timers.push({ resolve, delay }),
    console: { ...console, error: (...args) => errors.push(args) },
  });
  const store = exports.useAssetStore;
  const finish = (index, id) => requests[index].resolve({
    assets: [{ id }], total: 1, page: 0, pageSize: 100,
  });
  return { store, requests, timers, errors, finish };
}

test('Geç gelen eski filtre cevabı son seçilen filtrenin sonucunu ezmez', async () => {
  const { store, finish } = setup();
  store.getState().setFilter({ kinds: ['texture'] });
  const old = store.getState().search();
  store.getState().setFilter({ kinds: ['material'] });
  const current = store.getState().search();
  finish(1, 'material');
  await current;
  finish(0, 'texture');
  await old;
  assert.equal(store.getState().assets[0].id, 'material');
  assert.equal(store.getState().assetMap.has('texture'), false);
  assert.equal(store.getState().total, 1);
  assert.equal(store.getState().loading, false);
});

test('Eski istek tamamlanınca yeni isteğin yükleme durumu kapanmaz', async () => {
  const { store, finish } = setup();
  const old = store.getState().search();
  const current = store.getState().search();
  finish(0, 'old');
  await old;
  assert.equal(store.getState().loading, true);
  assert.equal(store.getState().assets.length, 0);
  finish(1, 'current');
  await current;
  assert.equal(store.getState().assets[0].id, 'current');
});

test('Yeni filtre beklerken eski kartlar kaldırılır; aynı sorgunun yenilemesinde korunur', async () => {
  const { store, finish } = setup();
  const initial = store.getState().search();
  finish(0, 'texture');
  await initial;
  const refresh = store.getState().search();
  assert.equal(store.getState().assets[0].id, 'texture');
  finish(1, 'texture');
  await refresh;
  store.getState().setFilter({ kinds: ['material'] });
  const filtered = store.getState().search();
  assert.equal(store.getState().assets.length, 0);
  assert.equal(store.getState().assetMap.size, 0);
  assert.equal(store.getState().total, 0);
  finish(2, 'material');
  await filtered;
});

test('Hızlı yazılan metinler yalnız son sorguyu veritabanına gönderir', async () => {
  const { store, requests, timers, finish } = setup();
  const pending = ['m', 'mu', 'mutfak'].map((text) => {
    store.getState().setQuery({ text });
    return store.getState().search(250);
  });
  assert.equal(requests.length, 0);
  assert.ok(timers.every((timer) => timer.delay === 250));
  timers.forEach((timer) => timer.resolve());
  await Promise.resolve();
  assert.equal(requests.length, 1);
  assert.equal(requests[0].payload.query.text, 'mutfak');
  finish(0, 'kitchen');
  await Promise.all(pending);
  assert.equal(store.getState().assets[0].id, 'kitchen');
});

test('Bekleyen metin aramasını filtre seçimi geçersiz kılar ve filtre hemen sorgulanır', async () => {
  const { store, requests, timers, finish } = setup();
  store.getState().setQuery({ text: 'wood' });
  const typed = store.getState().search(250);
  store.getState().setFilter({ kinds: ['material'] });
  const filtered = store.getState().search();
  assert.equal(requests.length, 1);
  assert.equal(requests[0].payload.query.filter.kinds[0], 'material');
  timers.forEach((timer) => timer.resolve());
  await typed;
  assert.equal(requests.length, 1);
  assert.equal(store.getState().loading, true);
  finish(0, 'material');
  await filtered;
});

test('Geçersiz kalan sorgunun hatası güncel sonucu ve hata durumunu değiştirmez', async () => {
  const { store, requests, errors, finish } = setup();
  const old = store.getState().search();
  const current = store.getState().search();
  finish(1, 'current');
  await current;
  requests[0].reject(new Error('Eski NAS isteği başarısız'));
  await old;
  assert.equal(store.getState().searchError, null);
  assert.equal(store.getState().assets[0].id, 'current');
  assert.equal(errors.length, 0);
});

test('Güncel sorgu hatası görünür; tekrar deneme başarıyla temizler', async () => {
  const { store, requests, errors, finish } = setup();
  const failed = store.getState().search();
  requests[0].reject(new Error('NAS bağlantısı kesildi'));
  await failed;
  assert.ok(store.getState().searchError);
  assert.equal(store.getState().loading, false);
  assert.equal(errors.length, 1);
  const retry = store.getState().search();
  assert.equal(store.getState().searchError, null);
  finish(1, 'recovered');
  await retry;
  assert.equal(store.getState().assets[0].id, 'recovered');
});

test('Sorgu veya sayfa değişince yeni istek başlamasa bile eski cevap uygulanmaz', async () => {
  for (const change of [
    (store) => store.getState().setQuery({ libraryId: 'another-library' }),
    (store) => store.getState().setPage(1),
  ]) {
    const { store, finish } = setup();
    const pending = store.getState().search();
    change(store);
    finish(0, 'obsolete');
    await pending;
    assert.equal(store.getState().assets.length, 0);
    assert.equal(store.getState().loading, false);
  }
});
