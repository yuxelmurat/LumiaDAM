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

function setup(overrides = {}) {
  const requests = [];
  const timers = [];
  const errors = [];
  const exports = {};
  const api = {
    searchAssets: (payload) => new Promise((resolve, reject) => {
      requests.push({ payload, resolve, reject });
    }),
    ...overrides,
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
  return { store, libraries: exports.useLibraryStore, ui: exports.useUIStore, requests, timers, errors, finish };
}

test('Kütüphane eklemek eski kapsamı temizler ve tıklama gerektirmeden tüm katalogları sorgular', async () => {
  const { store, libraries, ui, requests, finish } = setup({
    addLibrary: async (payload) => ({ id: 'new', name: 'A:', rootPaths: payload.rootPaths }),
  });
  ui.getState().setActiveLibrary('old');
  ui.getState().selectAsset('old-asset');
  store.getState().setQuery({ text: 'wood', libraryId: 'old', libraryIds: ['old'], collectionId: 'collection', filter: { kinds: ['texture'] } });
  store.getState().setPage(3);
  const adding = libraries.getState().addLibrary(['A:\\']);
  await new Promise(setImmediate);
  assert.equal(requests.length, 1);
  const { query, page } = requests[0].payload;
  assert.equal(query.libraryId, undefined);
  assert.equal(query.libraryIds, undefined);
  assert.equal(query.collectionId, undefined);
  assert.equal(query.text, '');
  assert.equal(Object.keys(query.filter).length, 0);
  assert.equal(page, 0);
  assert.equal(ui.getState().activeLibraryId, null);
  assert.equal(ui.getState().selectedLibraryIds.length, 0);
  assert.equal(ui.getState().selectedAssetIds.size, 0);
  finish(0, 'imported-asset');
  await adding;
  assert.equal(libraries.getState().libraries[0].name, 'A:');
  assert.equal(store.getState().assets[0].id, 'imported-asset');
});

test('Kütüphane eklenemezse mevcut görünüm korunur', async () => {
  const { store, libraries, ui, requests } = setup({
    addLibrary: async () => { throw new Error('Cannot open catalog'); },
  });
  ui.getState().setActiveLibrary('old');
  store.getState().setQuery({ libraryId: 'old' });
  await assert.rejects(libraries.getState().addLibrary(['A:\\']));
  assert.equal(ui.getState().activeLibraryId, 'old');
  assert.equal(store.getState().query.libraryId, 'old');
  assert.equal(libraries.getState().libraries.length, 0);
  assert.equal(requests.length, 0);
});

test('Klasöre girip geri dönmek metin aramasını ve kütüphane kapsamını geri getirir', async () => {
  const { store, ui, requests, finish } = setup();
  store.getState().setQuery({ text: 'kitchen', libraryIds: ['first', 'second'], filter: { extensions: ['jpg'], kinds: ['texture'] } });
  const folder = { libraryId: 'second', path: '2023/kitchen', fullPath: 'A:/Models/2023/kitchen', name: 'kitchen' };
  const entering = store.getState().openFolder(folder);
  assert.equal(requests[0].payload.query.text, '');
  assert.equal(requests[0].payload.query.folderPath, '2023/kitchen');
  assert.equal(requests[0].payload.query.libraryId, 'second');
  assert.equal(requests[0].payload.query.libraryIds, undefined);
  assert.deepEqual(Array.from(requests[0].payload.query.filter.extensions), ['jpg']);
  assert.deepEqual(Array.from(requests[0].payload.query.filter.kinds), ['texture']);
  assert.equal(ui.getState().activeLibraryId, 'second');
  finish(0, 'zip');
  await entering;
  const back = store.getState().backFromFolder();
  assert.equal(requests[1].payload.query.text, 'kitchen');
  assert.equal(requests[1].payload.query.folderPath, undefined);
  assert.deepEqual(Array.from(requests[1].payload.query.libraryIds), ['first', 'second']);
  finish(1, 'kitchen-file');
  await back;
  assert.equal(store.getState().folderHistory.length, 0);
  const again = store.getState().openFolder(folder);
  finish(2, 'zip');
  await again;
  store.getState().setQuery({ libraryId: undefined, libraryIds: undefined });
  assert.equal(store.getState().query.folderPath, undefined);
  assert.equal(store.getState().folderHistory.length, 0);
});

test('Koleksiyon filtresi klasöre girince ve geri dönünce görünür ve etkin kalır', async () => {
  const { store, ui, requests, finish } = setup();
  ui.getState().setActiveCollection('collection');
  store.getState().setQuery({ collectionId: 'collection', text: 'kitchen', filter: { extensions: ['jpg'] } });
  const folder = { libraryId: 'library', path: 'kitchen', fullPath: 'A:/kitchen', name: 'kitchen' };
  const entering = store.getState().openFolder(folder);
  assert.equal(requests[0].payload.query.collectionId, 'collection');
  assert.equal(requests[0].payload.query.libraryId, 'library');
  assert.equal(requests[0].payload.query.filter.extensions[0], 'jpg');
  assert.equal(ui.getState().activeCollectionId, 'collection');
  finish(0, 'file');
  await entering;
  const back = store.getState().backFromFolder();
  assert.equal(requests[1].payload.query.collectionId, 'collection');
  assert.equal(requests[1].payload.query.text, 'kitchen');
  assert.equal(ui.getState().activeCollectionId, 'collection');
  finish(1, 'file');
  await back;
});

test('Eski klasör sonuçları yeni aramayı ezmez; klasör sayfası metin değişince sıfırlanır', async () => {
  const { store, requests } = setup();
  store.getState().setQuery({ text: 'kitchen', folderPage: 2 });
  const old = store.getState().search();
  store.getState().setQuery({ text: 'salon' });
  const current = store.getState().search();
  assert.equal(requests[1].payload.query.folderPage, 0);
  requests[1].resolve({ assets: [], total: 0, folders: [{ name: 'salon' }], folderTotal: 1 });
  await current;
  requests[0].resolve({ assets: [], total: 0, folders: [{ name: 'kitchen' }], folderTotal: 10 });
  await old;
  assert.equal(store.getState().folders[0].name, 'salon');
  assert.equal(store.getState().folderTotal, 1);
});

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
