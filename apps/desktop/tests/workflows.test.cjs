const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');
const ts = require('typescript');
const compile = name => ts.transpileModule(readFileSync(join(__dirname, name), 'utf8'), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022 },
}).outputText;

test('Max aktarımı kaynak yolu yerine dosya kimliği ve seçilen Max oturumunu gönderir', async () => {
  const exports = {}, calls = [];
  runInNewContext(compile('../src/api/index.ts'), { exports, require: () => ({ invoke: async (command, payload) => {
    calls.push([command, payload]); return 'job-id';
  } }) });
  assert.equal(typeof exports.api.mergeAssetToMax, 'function', 'Max aktarım IPC çağrısı henüz yok');
  const result = await exports.api.mergeAssetToMax('asset-id', 'instance-id', 'C:/project/scene.max', 'C:/project');
  assert.equal(result, 'job-id');
  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [['merge_to_max', {
    assetId: 'asset-id', instanceId: 'instance-id', scenePath: 'C:/project/scene.max', projectPath: 'C:/project',
  }]]);
});

// Testlerde gerçek çeviri sözlükleri kullanılır; React aboneliği yalnız bu VM'de taklit edilir.
const localeCache = {};
function loadLocale(name = 'index') {
  if (localeCache[name]) return localeCache[name];
  const exports = {}; localeCache[name] = exports;
  runInNewContext(compile('../src/i18n/' + name + '.ts'), { exports, console,
    require: id => id === 'react' ? { useSyncExternalStore: (_subscribe, get) => get() }
      : id.startsWith('./') ? loadLocale(id.slice(2)) : {},
  });
  return exports;
}
const i18n = loadLocale();

// Max penceresi ve IPC dışında gerçek bileşenin hook ve eylemleri çalıştırılır.
function maxMergeHarness(found, result = {}) {
  const hooks = [], effects = [], calls = [], timers = new Map();
  let cursor = 0;
  const changed = (previous, next) => !previous || next.some((value, index) => !Object.is(value, previous[index]));
  const dialog = { showModal() {}, close() {} };
  const react = {
    useState: initial => {
      const index = cursor++;
      hooks[index] ??= { value: initial };
      return [hooks[index].value, next => { hooks[index].value = typeof next === 'function' ? next(hooks[index].value) : next; }];
    },
    useRef: initial => {
      const index = cursor++;
      return (hooks[index] ??= { current: index === 0 ? dialog : initial });
    },
    useEffect: (effect, deps) => {
      const index = cursor++;
      if (changed(hooks[index]?.deps, deps)) {
        hooks[index]?.cleanup?.(); hooks[index] = { deps };
        effects.push(() => { hooks[index].cleanup = effect(); });
      }
    },
  };
  const exports = {};
  const api = {
    getMaxInstances: async () => typeof found === 'function' ? found() : found,
    mergeAssetToMax: async (...args) => { calls.push(args); return 'job'; },
    getMaxMergeStatus: async () => ({ id: 'job', state: 'complete', phase: 'Merge complete', importedNodes: 1, ...result }),
  };
  runInNewContext(compile('../src/components/LibraryWorkflow/MaxMergeDialog.tsx'), { exports,
    setTimeout: callback => { const id = timers.size + 1; timers.set(id, callback); return id; },
    clearTimeout: id => timers.delete(id),
    require: name => {
      if (name.endsWith('/i18n')) return i18n;
      if (name === 'react') return react;
      if (name === 'react/jsx-runtime') return require(name);
      if (name === '../../api') return { api };
      if (name.endsWith('.css')) return { default: {} };
      return { open: async () => 'C:/project' };
    },
  });
  const props = { asset: { id: 'asset', extension: 'zip', fileName: 'chair.zip' }, open: true, onClose() {}, onBusyChange() {} };
  return {
    calls, timers, props,
    render: () => { cursor = 0; const tree = exports.MaxMergeDialog(props); effects.splice(0).forEach(effect => effect()); return tree; },
    cleanup: () => hooks.forEach(hook => hook?.cleanup?.()),
  };
}

function mergeButtons(node) {
  if (Array.isArray(node)) return node.flatMap(mergeButtons);
  if (!node || typeof node !== 'object') return [];
  return [...(node.type === 'button' ? [node] : []), ...mergeButtons(node.props?.children)];
}

const maxSession = { id: 'max', pid: 123, maxVersion: '28000', scenePath: 'C:/project/main.max', projectPath: 'C:/project', busy: false };

test('Tek kayıtlı Max sahnesine bir kez merge gönderilir; bağlantı yenileme tekrar merge etmez', async () => {
  const harness = maxMergeHarness([maxSession]);
  harness.render(); await new Promise(setImmediate);
  harness.render(); await new Promise(setImmediate);
  let tree = harness.render();
  assert.deepEqual(JSON.parse(JSON.stringify(harness.calls)), [['asset', 'max', 'C:/project/main.max', 'C:/project']]);
  const refresh = mergeButtons(tree).find(button => button.props.children === 'Refresh connection');
  refresh.props.onClick(); harness.render(); await new Promise(setImmediate);
  harness.render();
  assert.equal(harness.calls.length, 1);
  harness.cleanup();
});

test('Birden çok Max oturumunda veya kaydedilmemiş sahnede otomatik merge yapılmaz', async () => {
  for (const sessions of [[maxSession, { ...maxSession, id: 'second' }], [{ ...maxSession, scenePath: '' }], []]) {
    const harness = maxMergeHarness(sessions);
    harness.render(); await new Promise(setImmediate); harness.render();
    assert.equal(harness.calls.length, 0);
    harness.cleanup();
  }
});

test('Eksik materyal ve ışıklarla aktarımda açıklama görünür; uyarılar kapalı detayda saklanmaz', async () => {
  const harness = maxMergeHarness([maxSession], { incompleteMerge: true, phase: 'Incomplete import', warnings: ['VRayMtl unavailable'] });
  harness.render(); await new Promise(setImmediate); harness.render(); await new Promise(setImmediate);
  const tree = harness.render();
  const nodes = node => Array.isArray(node) ? node.flatMap(nodes) : !node || typeof node !== 'object' ? [] : [node, ...nodes(node.props?.children)];
  const content = node => typeof node === 'string' ? node : Array.isArray(node) ? node.map(content).join('') : node && typeof node === 'object' ? content(node.props?.children) : '';
  assert.match(content(tree), /Materials and lights are incomplete/);
  assert.match(content(tree), /unless you approve Corona conversion/);
  assert.equal(nodes(tree).find(node => node.type === 'details').props.open, true);
  harness.cleanup();
});

test('Kapatılmış merge bileşenine geç gelen bağlantı yanıtı sahneye dosya göndermez', async () => {
  let resolve;
  const harness = maxMergeHarness(() => new Promise(done => { resolve = done; }));
  harness.render(); harness.cleanup(); resolve([maxSession]); await new Promise(setImmediate);
  assert.equal(harness.calls.length, 0);
});

test('Dış uygulama açılırken dosya kimliği çözümlenir ve yerel kopyanın yolu kullanılır', async () => {
  const exports = {}, calls = [];
  runInNewContext(compile('../src/api/index.ts'), { exports, require: () => ({ invoke: async (command, payload) => {
    calls.push([command, payload]);
    if (command === 'resolve_asset_file') return 'C:/LumiaDAM/offline/scene.max';
  } }) });
  await exports.api.openAssetExternal({ id: 'scene', filePath: '//sleeping-nas/scene.max' });
  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [
    ['resolve_asset_file', { id: 'scene' }], ['open_with_default', { filePath: 'C:/LumiaDAM/offline/scene.max' }],
  ]);
});

test('Kaynak çözümlenemezse varsayılan uygulama başlatılmaz ve hata kaybolmaz', async () => {
  const exports = {}, calls = [];
  runInNewContext(compile('../src/api/index.ts'), { exports, require: () => ({ invoke: async command => {
    calls.push(command); throw new Error('Source unavailable');
  } }) });
  await assert.rejects(exports.api.openAssetExternal({ id: 'scene' }), /Source unavailable/);
  assert.deepEqual(calls, ['resolve_asset_file']);
});

function cadHarness(read, mount) {
  const effects = [], states = [], timers = new Map(), events = new Map();
  const exports = {};
  const host = { replaceChildren() {} };
  const react = { useRef: () => ({ current: host }), useEffect: effect => effects.push(effect), useState: initial => {
    const index = states.length; states.push(initial);
    return [initial, next => { states[index] = typeof next === 'function' ? next(states[index]) : next; }];
  } };
  const window = { setTimeout: callback => { const id = timers.size + 1; timers.set(id, callback); return id; }, clearTimeout: id => timers.delete(id),
    addEventListener: (name, fn) => events.set(name, fn), removeEventListener: name => events.delete(name) };
  runInNewContext(compile('../src/components/CadViewer/CadViewer.tsx'), { exports, window, Uint8Array, ArrayBuffer, Error,
    require: name => {
      if (name.endsWith('/i18n')) return i18n;
      if (name === 'react') return react;
      if (name === 'react/jsx-runtime') return require(name);
      if (name === '@tauri-apps/api/core') return { invoke: read };
      if (name === '@exav/viewer') return { createViewer: () => ({ mount }) };
      if (name === '@exav/viewer/cad') return { dwg: () => ({}), dxf: () => ({}), isCadSupported: () => true };
      if (name.endsWith('.css')) return { default: {} };
      return new Proxy({}, { get: () => () => null });
    },
  });
  const asset = { id: 'drawing', extension: 'dwg', fileName: '9.dwg', filePath: '//nas/9.dwg', modifiedAt: 1, sizeBytes: 10 };
  const tree = exports.CadViewer({ asset, onBack: () => events.set('back', true) });
  const cleanup = effects.map(effect => effect());
  return { states, timers, events, tree, cleanup: () => cleanup.forEach(fn => fn?.()) };
}

test('CAD yükleme hatası görünür kalır ve geri dönüş düğmesi korunur', async () => {
  const harness = cadHarness(async () => { throw new Error('Source unavailable'); }, () => { throw new Error('Must not mount'); });
  await new Promise(setImmediate);
  assert.equal(harness.states[1], true);
  assert.match(harness.states[0], /Source unavailable/);
  assert.equal(harness.timers.size, 0);
  const back = harness.tree.props.children[0].props.children[0];
  back.props.onClick(); assert.equal(harness.events.get('back'), true);
  harness.cleanup();
});

test('Kapatılan CAD ekranına geç gelen dosya yanıtı görüntüleyici başlatmaz', async () => {
  let resolve, mounts = 0;
  const harness = cadHarness(() => new Promise(done => { resolve = done; }), () => { mounts++; });
  harness.cleanup(); resolve(new ArrayBuffer(10)); await new Promise(setImmediate);
  assert.equal(mounts, 0); assert.equal(harness.timers.size, 0);
});

test('CAD motoru başlatılırken hata verirse yeniden denemeye uygun hata durumu oluşur', async () => {
  const harness = cadHarness(async () => new ArrayBuffer(10), () => { throw new Error('CAD engine failed'); });
  await new Promise(setImmediate);
  assert.equal(harness.states[1], true); assert.match(harness.states[0], /CAD engine failed/);
  harness.cleanup();
});

// Gerçek klasör bileşenlerini çalıştırır; yalnız React hook yaşam döngüsü ve IPC denetlenir.
function folderHarness(component, initialProps, pending = []) {
  const hooks = [], effects = [];
  let cursor = 0, writes = 0, props = initialProps;
  const changed = (previous, next) => !previous || next.some((value, index) => !Object.is(value, previous[index]));
  const react = {
    useState: initial => {
      const index = cursor++;
      hooks[index] ??= { value: typeof initial === 'function' ? initial() : initial };
      return [hooks[index].value, next => { writes++; hooks[index].value = typeof next === 'function' ? next(hooks[index].value) : next; }];
    },
    useRef: initial => { const index = cursor++; return (hooks[index] ??= { current: initial }); },
    useCallback: (callback, deps) => {
      const index = cursor++;
      if (changed(hooks[index]?.deps, deps)) hooks[index] = { value: callback, deps };
      return hooks[index].value;
    },
    useEffect: (effect, deps) => {
      const index = cursor++;
      if (changed(hooks[index]?.deps, deps)) {
        hooks[index]?.cleanup?.(); hooks[index] = { deps };
        effects.push(() => { hooks[index].cleanup = effect(); });
      }
    },
  };
  const mockedApi = { searchAssets: payload => new Promise((resolve, reject) => pending.push({ payload, resolve, reject })) };
  const stores = {};
  if (component === 'Sidebar') {
    const original = {};
    runInNewContext(compile('../src/store/index.ts'), { exports: original,
      require: name => name === '../api' ? { api: mockedApi } : require(name), setTimeout, console });
    original.useLibraryStore.setState({ libraries: [dummyLibrary, { ...dummyLibrary, id: 'second', name: 'B:', rootPaths: ['B:/'] }] });
    for (const [name, store] of Object.entries(original)) {
      stores[name] = Object.assign((selector = state => state) => selector(store.getState()), { getState: store.getState });
    }
  }
  const exports = {};
  runInNewContext(compile('../src/components/Sidebar/Sidebar.tsx'), { exports,
    require: name => {
      if (name.endsWith('/i18n')) return i18n;
      if (name === 'react') return react;
      if (name === 'react/jsx-runtime') return require(name);
      if (name === 'clsx') return { default: require(name) };
      if (name === '../../api') return { api: mockedApi };
      if (name === '../../store') return stores;
      if (name.endsWith('.css')) return { default: new Proxy({}, { get: (_, key) => key }) };
      return new Proxy({}, { get: () => () => null });
    },
  });
  const render = next => {
    props = next ?? props; cursor = 0;
    const tree = exports[component ?? 'LibraryFolderChildren'](props);
    effects.splice(0).forEach(effect => effect());
    return tree;
  };
  return { render, pending, stores, cleanup: () => hooks.forEach(hook => hook?.cleanup?.()), writes: () => writes };
}

const dummyLibrary = { id: 'library', name: 'A:', rootPaths: ['A:/'], assetCount: 1, createdAt: 1 };
const dummyFolder = (path, libraryId = 'library') => ({ libraryId, path, name: path.split('/').pop(), fullPath: `A:/${path}` });
const folderResponse = (folders, total = folders.length) => ({ folders, folderTotal: total, assets: [], total: 0 });
const settleFolders = () => new Promise(setImmediate);
const folderNodes = tree => tree.props.children[0];
function folderButton(tree, text) {
  if (!tree || typeof tree !== 'object') return undefined;
  if (tree.type === 'button' && tree.props.children === text) return tree;
  return [tree.props?.children].flat(Infinity).map(child => folderButton(child, text)).find(Boolean);
}

function sidebarLibrary(tree, id) {
  if (!tree || typeof tree !== 'object') return undefined;
  if (tree.props?.id === `lib-${id}`) return tree;
  return [tree.props?.children].flat(Infinity).map(child => sidebarLibrary(child, id)).find(Boolean);
}

test('Kütüphane tıklaması seçimi korur; Ctrl çoklu seçim yapar ve ok yalnız ağacı değiştirir', () => {
  const harness = folderHarness('Sidebar', {});
  let tree = harness.render();
  const click = ctrlKey => ({ target: { tagName: 'DIV' }, ctrlKey });
  sidebarLibrary(tree, 'library').props.onClick(click(false));
  tree = harness.render();
  sidebarLibrary(tree, 'second').props.onClick(click(true));
  assert.deepEqual(Array.from(harness.stores.useUIStore.getState().selectedLibraryIds), ['library', 'second']);
  tree = harness.render();
  sidebarLibrary(tree, 'library').props.onClick(click(false));
  sidebarLibrary(tree, 'library').props.onClick(click(false));
  assert.deepEqual(Array.from(harness.stores.useUIStore.getState().selectedLibraryIds), ['library']);
  tree = harness.render();
  const before = harness.stores.useAssetStore.getState().query;
  sidebarLibrary(tree, 'second').props.children[0].props.onClick({ stopPropagation() {} });
  assert.equal(harness.stores.useAssetStore.getState().query, before);
  assert.deepEqual(Array.from(harness.stores.useUIStore.getState().selectedLibraryIds), ['library']);
  harness.cleanup();
});

test('Sol ağaç yalnız katalogdaki doğrudan alt klasörleri sayfalayarak yükler', async () => {
  const harness = folderHarness(null, { library: dummyLibrary, parentPath: '', onNavigate() {} });
  harness.render();
  const query = harness.pending[0].payload;
  assert.equal(query.resultScope, 'folders');
  assert.equal(query.query.folderPath, '');
  assert.equal(query.query.libraryId, 'library');
  assert.equal(query.query.text, '');
  assert.equal(Object.keys(query.query.filter).length, 0);
  const first = Array.from({ length: 100 }, (_, i) => dummyFolder(`folder-${i}`));
  harness.pending[0].resolve(folderResponse(first, 101)); await settleFolders();
  const tree = harness.render();
  assert.equal(folderNodes(tree).length, 100);
  folderButton(tree, 'Show more folders').props.onClick();
  assert.equal(harness.pending[1].payload.query.folderPage, 1);
  harness.pending[1].resolve(folderResponse([dummyFolder('last-folder')], 101)); await settleFolders();
  const complete = harness.render();
  assert.equal(folderNodes(complete).length, 101);
  assert.equal(folderButton(complete, 'Show more folders'), undefined);
  harness.cleanup();
});

test('Klasör seçimi doğru yolu açar; okla genişletme galerinin seçimini değiştirmez', async () => {
  const root = folderHarness(null, { library: dummyLibrary, parentPath: '', onNavigate() {} });
  root.render(); root.pending[0].resolve(folderResponse([dummyFolder('kitchen')])); await settleFolders();
  const node = folderNodes(root.render())[0], selected = [];
  const harness = folderHarness('LibraryFolderNode', { ...node.props, onNavigate: folder => selected.push(folder) });
  let tree = harness.render();
  assert.equal(tree.props.children[1], false);
  assert.equal(harness.pending.length, 0);
  tree.props.children[0].props.children[0].props.onClick();
  tree = harness.render();
  assert.equal(selected.length, 0);
  assert.equal(tree.props.children[1].props.parentPath, 'kitchen');
  tree.props.children[0].props.children[1].props.onClick();
  assert.equal(selected[0].path, 'kitchen');
  assert.equal(selected[0].libraryId, 'library');
  harness.cleanup(); root.cleanup();
});

test('Eski veya kapatılmış klasör dalının yanıtı yeni kütüphane listesini değiştirmez', async () => {
  const props = { library: dummyLibrary, parentPath: '', onNavigate() {} };
  const harness = folderHarness(null, props);
  harness.render(); harness.render({ ...props, library: { ...dummyLibrary, id: 'new-library' } });
  harness.pending[1].resolve(folderResponse([dummyFolder('current', 'new-library')])); await settleFolders();
  harness.pending[0].resolve(folderResponse([dummyFolder('old')])); await settleFolders();
  assert.equal(folderNodes(harness.render())[0].props.folder.path, 'current');
  harness.render({ ...props, library: { ...dummyLibrary, id: 'closed-library' } });
  harness.cleanup();
  const before = harness.writes();
  harness.pending[2].resolve(folderResponse([dummyFolder('late', 'closed-library')])); await settleFolders();
  assert.equal(harness.writes(), before);
});

test('Katalog yenileme hatasında mevcut klasörler korunur ve aynı sayfa yeniden denenir', async () => {
  const props = { library: dummyLibrary, parentPath: '', onNavigate() {} };
  const harness = folderHarness(null, props);
  harness.render(); harness.pending[0].resolve(folderResponse([dummyFolder('cached')])); await settleFolders();
  const refreshed = { ...props, library: { ...dummyLibrary } };
  harness.render(refreshed); harness.pending[1].reject(new Error('Catalog temporarily unavailable')); await settleFolders();
  const failed = harness.render();
  assert.equal(folderNodes(failed)[0].props.folder.path, 'cached');
  folderButton(failed, 'Retry').props.onClick();
  assert.equal(harness.pending[2].payload.query.folderPage, 0);
  harness.pending[2].resolve(folderResponse([dummyFolder('fresh')])); await settleFolders();
  assert.equal(folderNodes(harness.render())[0].props.folder.path, 'fresh');
  harness.cleanup();
});

test('Bozuk indeks başka kütüphaneyi veya üst klasörü ağaçta alt dal yapamaz', async () => {
  const harness = folderHarness(null, { library: dummyLibrary, parentPath: 'kitchen', onNavigate() {} });
  harness.render();
  harness.pending[0].resolve(folderResponse([dummyFolder('kitchen'), dummyFolder('outside'),
    dummyFolder('kitchen/foreign', 'other-library'), dummyFolder('kitchen/corona')])); await settleFolders();
  const nodes = folderNodes(harness.render());
  assert.equal(nodes.length, 1);
  assert.equal(nodes[0].props.folder.path, 'kitchen/corona');
  harness.cleanup();
});

test('Türkçe Max ekranı dosya yollarını ve aktarım komutunu değiştirmez; dil değişimi yeniden merge etmez', async () => {
  await i18n.setLanguage('tr');
  const harness = maxMergeHarness([maxSession]);
  const content = node => typeof node === 'string' ? node : Array.isArray(node) ? node.map(content).join('') : node && typeof node === 'object' ? content(node.props?.children) : '';
  try {
    harness.render(); await new Promise(setImmediate); harness.render(); await new Promise(setImmediate);
    const turkish = content(harness.render());
    assert.match(turkish, /Bağlantıyı yenile/);
    assert.match(turkish, /chair\.zip/);
    assert.equal(harness.calls.length, 1);
    assert.equal(harness.calls[0][3], 'C:/project');
    await i18n.setLanguage('en');
    assert.match(content(harness.render()), /Refresh connection/);
    assert.equal(harness.calls.length, 1);
  } finally { harness.cleanup(); await i18n.setLanguage('en'); }
});
