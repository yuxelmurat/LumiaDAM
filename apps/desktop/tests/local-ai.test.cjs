const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync, existsSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');
const ts = require('typescript');
const compile = file => {
  const path = join(__dirname, '../src', file);
  assert.ok(existsSync(path), `${file} bileşeni henüz yok`);
  return ts.transpileModule(readFileSync(path, 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022 },
  }).outputText;
};
const plain = value => JSON.parse(JSON.stringify(value));
const settle = () => new Promise(setImmediate);
const settings = { mode: 'disabled', provider: 'bundled', endpoint: 'http://127.0.0.1:11434',
  model: 'qwen3.5:0.8b', idleMinutes: 10, startHour: 1, endHour: 6, cpuOnly: true, batchSize: 100, setupComplete: false };
const overview = value => ({ settings: { ...settings, ...value }, runtimeInstalled: true, installRequested: true });
const status = value => ({ state: 'idle', phase: 'Ready', processed: 0, total: 0, suggestions: 0,
  error: null, running: false, engineRunning: false, modelLoaded: false, lastRunAt: null, ...value });

// Üretimdeki sözlük ve çevirici kullanılır; yalnız React/IPC/depolama sınırı taklit edilir.
function translations(language = 'en') {
  const exports = {};
  runInNewContext(compile('i18n/index.ts'), { exports, console,
    document: { documentElement: { lang: 'en' } }, localStorage: { setItem() {}, getItem: () => language },
    require: dependency => {
      if (dependency === 'react') return { useSyncExternalStore: (_subscribe, snapshot) => snapshot() };
      if (dependency === '@tauri-apps/api/core') return { invoke: async () => language };
      const dictionary = {};
      runInNewContext(compile(`i18n/${dependency.slice(2)}.ts`), { exports: dictionary });
      return dictionary;
    },
  });
  void exports.setLanguage(language);
  return exports;
}

// React hook'ları ve IPC sınırı taklit edilir; bileşenin kendi eylemleri gerçekten çalışır.
function harness(file, name, options = {}) {
  const hooks = [], effects = [], calls = [], timers = new Map(), exports = {};
  const i18n = translations(options.language);
  let cursor = 0, timerId = 0, writes = 0;
  const changed = (a, b) => !a || !b || a.length !== b.length || b.some((value, i) => !Object.is(a[i], value));
  const react = {
    useState: initial => { const i = cursor++; hooks[i] ??= { value: typeof initial === 'function' ? initial() : initial };
      return [hooks[i].value, value => { writes++; hooks[i].value = typeof value === 'function' ? value(hooks[i].value) : value; }]; },
    useRef: initial => { const i = cursor++; return (hooks[i] ??= { current: initial }); },
    useEffect: (effect, deps) => { const i = cursor++; if (changed(hooks[i]?.deps, deps)) {
      hooks[i]?.cleanup?.(); hooks[i] = { deps }; effects.push(() => { hooks[i].cleanup = effect(); });
    } },
    useCallback: (fn, deps) => { const i = cursor++; if (changed(hooks[i]?.deps, deps)) hooks[i] = { deps, fn }; return hooks[i].fn; },
    lazy: () => 'LazySettings', Suspense: 'Suspense',
  };
  const api = {};
  const defaults = { getLocalAiSettings: () => overview(options.settings), getLocalAiStatus: () => status(),
    listAiSuggestions: () => [], saveLocalAiSettings: next => overview(next),
    getLocalAiLog: () => ({ path: 'C:/Local/activity.jsonl', entries: [] }), openLocalAiLog() {},
    startLocalAi() {}, stopLocalAi() {}, prepareLocalAiModel() {}, applyAiSuggestion() {}, applyAllAiSuggestions: () => ({ approved: 0, failed: 0 }), dismissAiSuggestion() {} };
  for (const [method, fallback] of Object.entries(defaults)) api[method] = async (...args) => {
    calls.push([method, ...args]); return (options.api?.[method] ?? fallback)(...args);
  };
  const store = Object.assign(() => ({}), { getState: () => ({
    fetchCollections: async () => { calls.push(['fetchCollections']); },
    fetchTags: async () => { calls.push(['fetchTags']); },
  }) });
  const stores = { useUIStore: store, useAssetStore: store, useLibraryStore: store, useTagStore: store, useCollectionStore: store };
  if (options.app) {
    const fetch = async () => {};
    stores.useLibraryStore = fn => fn({ fetchLibraries: fetch });
    stores.useTagStore = fn => fn({ fetchTags: fetch });
    stores.useCollectionStore = fn => fn({ fetchCollections: fetch });
    stores.useAssetStore = fn => fn({ search: fetch });
  }
  const window = { setTimeout: (fn, delay) => { timers.set(++timerId, { fn, delay }); return timerId; },
    clearTimeout: id => timers.delete(id), addEventListener() {}, removeEventListener() {} };
  runInNewContext(compile(file), { exports, window, setTimeout: window.setTimeout, clearTimeout: window.clearTimeout,
    URL, console, document: { activeElement: null }, require: dependency => {
      if (dependency === 'react') return react;
      if (dependency === 'clsx') return { default: (...values) => values.filter(Boolean).join(' ') };
      if (dependency === '@tauri-apps/api/core') return { convertFileSrc: path => `asset://${path}` };
      if (dependency === 'react/jsx-runtime') return require(dependency);
      if (dependency.endsWith('/api')) return { api };
      if (dependency.endsWith('/store')) return stores;
      if (dependency.endsWith('/i18n')) return i18n;
      if (dependency === '@tauri-apps/api/event') return { listen: async () => () => {} };
      if (dependency.endsWith('.css')) return { default: {} };
      return new Proxy({}, { get: (_, key) => key === 'editors' ? {} : key });
    } });
  return { calls, timers, i18n, writes: () => writes,
    render: (props = {}) => { cursor = 0; const tree = exports[name](props); effects.splice(0).forEach(effect => effect()); return tree; },
    cleanup: () => hooks.forEach(hook => hook?.cleanup?.()),
    tick: async () => { const [id, timer] = timers.entries().next().value ?? []; if (timer) { timers.delete(id); await timer.fn(); await settle(); } },
  };
}
function nodes(node, predicate = () => true) {
  if (Array.isArray(node)) return node.flatMap(child => nodes(child, predicate));
  if (!node || typeof node !== 'object') return [];
  return [...(predicate(node) ? [node] : []), ...nodes(node.props?.children, predicate)];
}
const text = node => typeof node === 'string' || typeof node === 'number' ? String(node) :
  Array.isArray(node) ? node.map(text).join('') : node && typeof node === 'object' ? text(node.props?.children) : '';
const button = (tree, label) => nodes(tree, n => n.type === 'button' && text(n) === label)[0];
const input = (tree, id) => nodes(tree, n => n.props?.id === id)[0];
const reviewItems = tree => nodes(nodes(tree, n => n.props?.['aria-labelledby'] === 'local-ai-review')[0], n => n.type === 'li');
async function ready(h) { h.render(); await settle(); h.render(); await settle(); return h.render(); }

test('Local AI API yalnız seçilen komutları ve onaylanacak önerinin kimliğini gönderir', async () => {
  const exports = {}, calls = [];
  runInNewContext(compile('api/index.ts'), { exports, require: () => ({ invoke: async (...args) => { calls.push(args); } }) });
  const expected = [['getLocalAiSettings', 'get_local_ai_settings'], ['getLocalAiStatus', 'get_local_ai_status'],
    ['startLocalAi', 'start_local_ai'], ['stopLocalAi', 'stop_local_ai'], ['prepareLocalAiModel', 'prepare_local_ai_model'],
    ['listAiSuggestions', 'list_ai_suggestions'], ['applyAllAiSuggestions', 'apply_all_ai_suggestions']];
  for (const [method] of expected) assert.equal(typeof exports.api[method], 'function', `${method} IPC bağlantısı henüz yok`);
  for (const [method] of expected) await exports.api[method]();
  await exports.api.saveLocalAiSettings(settings);
  await exports.api.applyAiSuggestion('suggestion-id');
  await exports.api.dismissAiSuggestion('suggestion-id');
  assert.deepEqual(plain(calls), [...expected.map(([, command]) => [command]),
    ['save_local_ai_settings', { settings }], ['apply_ai_suggestion', { id: 'suggestion-id' }], ['dismiss_ai_suggestion', { id: 'suggestion-id' }]]);
});

test('Kapalı ve manuel modlar panelde kendiliğinden tarama, model indirme veya sürekli sorgu başlatmaz', async () => {
  for (const mode of ['disabled', 'manual']) {
    const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { settings: { mode, setupComplete: true } });
    const tree = await ready(h);
    assert.equal(h.calls.filter(([name]) => name === 'getLocalAiSettings').length, 1);
    assert.equal(h.timers.size, 0);
    assert.equal(h.calls.some(([name]) => ['startLocalAi', 'prepareLocalAiModel'].includes(name)), false);
    assert.equal(button(tree, 'Run now').props.disabled, mode === 'disabled');
    assert.equal(nodes(tree, n => n.type === 'textarea' || n.props?.type === 'search').length, 0);
    h.cleanup();
  }
});

test('İlk kurulumda dört açık seçim vardır; kaydetmek kapalı modu onaylar ve model başlatmaz', async () => {
  const h = harness('components/LocalAISettings.tsx', 'LocalAISettings');
  const tree = await ready(h);
  const choices = nodes(tree, n => n.type === 'input' && n.props.type === 'radio');
  assert.deepEqual(choices.map(n => n.props.value), ['disabled', 'manual', 'idle', 'scheduled']);
  assert.equal(choices[0].props.checked, true);
  await button(tree, 'Save settings').props.onClick(); await settle();
  assert.equal(h.calls.find(([name]) => name === 'saveLocalAiSettings')[1].setupComplete, true);
  assert.equal(h.calls.some(([name]) => name === 'startLocalAi'), false);
  h.cleanup();
});

test('Geçersiz uzak endpoint ve eşit saatler kaydedilmez; gece yarısını aşan saat aralığı geçerlidir', async () => {
  const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { settings: { provider: 'ollama', mode: 'scheduled' } });
  let tree = await ready(h);
  input(tree, 'local-ai-endpoint').props.onChange({ target: { value: 'https://remote.example:11434' } });
  tree = h.render(); assert.equal(button(tree, 'Save settings').props.disabled, true);
  input(tree, 'local-ai-endpoint').props.onChange({ target: { value: 'http://localhost:11434' } });
  input(h.render(), 'local-ai-start').props.onChange({ target: { value: '6' } });
  tree = h.render(); assert.equal(button(tree, 'Save settings').props.disabled, true);
  input(tree, 'local-ai-start').props.onChange({ target: { value: '22' } });
  tree = h.render(); assert.equal(button(tree, 'Save settings').props.disabled, false);
  h.cleanup();
});

test('Model indirme ve manuel çalıştırma yalnız ilgili düğmeye basılınca olur; durdurma açıkça schedulerı da durdurur', async () => {
  const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { settings: { mode: 'manual', setupComplete: true } });
  let tree = await ready(h);
  await button(tree, 'Download model').props.onClick(); await settle();
  assert.equal(h.calls.filter(([name]) => name === 'prepareLocalAiModel').length, 1);
  assert.equal(h.calls.some(([name]) => name === 'startLocalAi'), false);
  assert.match(text(h.render()), /Model download started/);
  assert.doesNotMatch(text(h.render()), /Model download complete/);
  tree = h.render(); await button(tree, 'Run now').props.onClick(); await settle();
  assert.equal(h.calls.filter(([name]) => name === 'startLocalAi').length, 1);
  tree = h.render(); await button(tree, 'Stop and pause automatic runs').props.onClick(); await settle();
  assert.equal(h.calls.filter(([name]) => name === 'stopLocalAi').length, 1);
  assert.match(text(h.render()), /Save settings to resume/i);
  h.cleanup();
});

test('Açık panel çalışan işi izler; kapatma timerı temizler ve geç yanıtı yok sayar', async () => {
  let finish;
  const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { settings: { mode: 'idle' },
    api: { getLocalAiStatus: () => new Promise(resolve => { finish = resolve; }) } });
  h.render(); await settle(); h.render();
  h.cleanup(); const before = h.writes(); finish(status({ running: true })); await settle();
  assert.equal(h.writes(), before); assert.equal(h.timers.size, 0);
  const active = harness('components/LocalAISettings.tsx', 'LocalAISettings', { settings: { mode: 'manual' },
    api: { getLocalAiStatus: () => status({ running: true }) } });
  await ready(active); assert.equal(active.timers.size, 1);
  await active.tick(); assert.equal(active.calls.filter(([name]) => name === 'getLocalAiStatus').length, 2);
  active.cleanup(); assert.equal(active.timers.size, 0);
});

test('Öneriler dosya adlarıyla incelenir; onay/dismiss yalnız seçilen öneriye uygulanır ve hatalar görünür', async () => {
  const suggestion = { id: 'candidate', kind: 'duplicates', title: 'Chair candidates', reason: 'Similar previews', assetIds: ['asset'],
    collectionName: 'Chair review', status: 'pending', items: [{ id: 'asset', fileName: 'chair.max', filePath: '//nas/chair.max', libraryName: 'Furniture' }] };
  const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { api: { listAiSuggestions: () => [suggestion],
    applyAiSuggestion: () => { throw new Error('Source unavailable'); } } });
  let tree = await ready(h);
  assert.equal(typeof nodes(tree, n => n.type === 'details')[0].props.onToggle, 'function', 'Detay açma eylemi henüz bağlanmamış');
  nodes(tree, n => n.type === 'details')[0].props.onToggle({ currentTarget: { open: true } });
  tree = h.render();
  assert.match(text(tree), /chair\.max/); assert.match(text(tree), /Furniture/);
  await button(tree, 'Approve grouping').props.onClick(); await settle();
  assert.deepEqual(h.calls.find(([name]) => name === 'applyAiSuggestion'), ['applyAiSuggestion', 'candidate']);
  tree = h.render(); assert.match(text(tree), /Source unavailable/);
  await button(tree, 'Dismiss').props.onClick(); await settle();
  assert.deepEqual(h.calls.find(([name]) => name === 'dismissAiSuggestion'), ['dismissAiSuggestion', 'candidate']);
  h.cleanup();
});

test('Uygulama açılışı yalnız küçük ayar kaydını okur; installer isteği varsa ilk seçimi açar', async () => {
  for (const [installRequested, setupComplete, shouldOpen] of [[true, false, true], [false, false, false], [true, true, false]]) {
    const h = harness('App.tsx', 'default', { app: true,
      api: { getLocalAiSettings: () => ({ ...overview({ setupComplete }), installRequested }) } });
    await ready(h);
    const tree = h.render();
    assert.deepEqual(h.calls.map(([name]) => name), ['getLocalAiSettings']);
    assert.equal(nodes(tree, n => n.props?.workspace === 'local-ai').length > 0, shouldOpen);
    assert.equal(h.timers.size, 0); h.cleanup();
  }
});

test('İndirme beklerken durdurma kullanılabilir; geç dönen indirme başarı mesajını geri getirmez', async () => {
  let finish;
  const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { settings: { mode: 'manual', setupComplete: true },
    api: { prepareLocalAiModel: () => new Promise(resolve => { finish = resolve; }) } });
  let tree = await ready(h);
  const download = button(tree, 'Download model').props.onClick();
  tree = h.render();
  const stop = button(tree, 'Stop and pause automatic runs');
  assert.equal(stop.props.disabled, false, 'İndirme sırasında durdurma etkin olmalı');
  await stop.props.onClick(); await settle();
  finish(); await download; await settle();
  assert.match(text(h.render()), /save settings to resume/i);
  assert.doesNotMatch(text(h.render()), /Model download started/);
  h.cleanup();
});

test('Başarılı öneri onayı katalog koleksiyonlarını yeniler; kaynak dosya eylemi göndermez', async () => {
  let pending = true;
  const suggestion = { id: 'collection', kind: 'collection', title: 'Chairs', reason: 'Seat geometry', assetIds: ['asset'],
    collectionName: 'Chairs', status: 'pending', items: [{ id: 'asset', fileName: 'chair.max', filePath: '//nas/chair.max', libraryName: 'Models' }] };
  const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { api: {
    listAiSuggestions: () => pending ? [suggestion] : [], applyAiSuggestion: () => { pending = false; },
  } });
  const tree = await ready(h);
  await button(tree, 'Approve collection').props.onClick(); await settle();
  assert.equal(h.calls.filter(([name]) => name === 'applyAiSuggestion').length, 1);
  assert.equal(h.calls.filter(([name]) => name === 'fetchCollections').length, 1);
  assert.equal(h.calls.filter(([name]) => name === 'fetchTags').length, 0);
  assert.match(text(h.render()), /No pending suggestions/);
  h.cleanup();
});

test('Tüm bekleyen AI önerileri tek eylemle kabul edilir ve koleksiyonlar bir kez yenilenir', async () => {
  let pending = true;
  const suggestions = [
    { id: 'category:Kitchen', kind: 'collection', collectionName: 'Kitchen', status: 'pending', items: [] },
    { id: 'category:Bedroom', kind: 'collection', collectionName: 'Bedroom', status: 'pending', items: [] },
  ];
  const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { api: {
    listAiSuggestions: () => pending ? suggestions : [],
    applyAllAiSuggestions: () => { pending = false; return { approved: 2, failed: 1 }; },
  } });
  let tree = await ready(h);
  const acceptAll = button(tree, 'Accept all pending suggestions');
  assert.ok(acceptAll, 'Bekleyen öneriler için toplu kabul düğmesi bulunmalı');
  await acceptAll.props.onClick(); await settle();
  tree = h.render();
  assert.equal(h.calls.filter(([name]) => name === 'applyAllAiSuggestions').length, 1);
  assert.equal(h.calls.filter(([name]) => name === 'applyAiSuggestion').length, 0);
  assert.equal(h.calls.filter(([name]) => name === 'fetchCollections').length, 1);
  assert.match(text(tree), /2 suggestions approved; 1 could not be added/);
  assert.match(text(tree), /No pending suggestions/);
  h.cleanup();
});

test('LM Studio kullanımında indirme düğmesi yoktur ve GPU seçimini mevcut server yönetir', async () => {
  const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { settings: {
    provider: 'lmstudio', model: 'existing-model', endpoint: 'http://localhost:1234', mode: 'manual', setupComplete: true,
  } });
  let tree = await ready(h);
  assert.equal(button(tree, 'Download model'), undefined);
  assert.equal(nodes(tree, n => n.type === 'input' && n.props.type === 'checkbox')[0].props.disabled, true);
  input(tree, 'local-ai-model').props.onChange({ target: { value: '' } });
  tree = h.render(); assert.equal(button(tree, 'Save settings').props.disabled, true);
  assert.equal(button(tree, 'Run now').props.disabled, true);
  h.cleanup();
});

test('Yerel endpoint yalnız HTTP kök adresini kabul eder; HTTPS, API yolu ve sıfır portu reddedilir', async () => {
  const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { settings: { provider: 'lmstudio', model: 'vision', mode: 'manual' } });
  let tree = await ready(h);
  for (const value of ['https://localhost:1234', 'http://localhost:1234/v1', 'http://localhost:1234/v1/', 'http://127.0.0.1:0']) {
    input(tree, 'local-ai-endpoint').props.onChange({ target: { value } });
    tree = h.render(); assert.equal(button(tree, 'Save settings').props.disabled, true, value);
  }
  for (const value of ['http://localhost:1234', 'http://127.0.0.1:1234/', 'http://127.2.3.4:1234', 'http://[::1]:1234']) {
    input(tree, 'local-ai-endpoint').props.onChange({ target: { value } });
    tree = h.render(); assert.equal(button(tree, 'Save settings').props.disabled, false, value);
  }
  assert.match(text(tree), /Load a vision model in LM Studio/);
  assert.match(text(tree), /does not load or unload LM Studio models/);
  h.cleanup();
});

test('Durdurma sonrasında backend ayarları okunur ve kalıcı Manual modu otomatik modun yerini alır', async () => {
  let stopped = false;
  const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { api: {
    getLocalAiSettings: () => overview({ mode: stopped ? 'manual' : 'idle', setupComplete: true }),
    stopLocalAi: () => { stopped = true; },
  } });
  let tree = await ready(h);
  assert.equal(h.timers.size, 1);
  await button(tree, 'Stop and pause automatic runs').props.onClick(); await settle();
  tree = h.render(); await settle(); tree = h.render();
  assert.equal(h.calls.filter(([name]) => name === 'getLocalAiSettings').length, 2);
  assert.equal(nodes(tree, n => n.type === 'input' && n.props.type === 'radio' && n.props.value === 'manual')[0].props.checked, true);
  assert.equal(h.timers.size, 0);
  assert.match(text(tree), /choose When idle or Scheduled/i);
  assert.match(text(tree), /This feature never moves or deletes source files/i);
  h.cleanup();
});

test('Disabled modunda veya ilk seçim kaydedilmeden model indirmesi ve çalışma başlatılamaz', async () => {
  for (const value of [{ mode: 'disabled', setupComplete: true }, { mode: 'manual', setupComplete: false }]) {
    const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { settings: value });
    const tree = await ready(h);
    assert.equal(button(tree, 'Download model').props.disabled, true);
    assert.equal(button(tree, 'Run now').props.disabled, true);
    h.cleanup();
  }
});

test('Harici sağlayıcıların motor sahipliği açık gösterilir; indirme ilerlemesi MiB birimindedir', async () => {
  for (const [provider, owner] of [['ollama', 'Ollama'], ['lmstudio', 'LM Studio']]) {
    const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { settings: {
      provider, mode: 'manual', setupComplete: true, model: provider === 'lmstudio' ? 'vision' : settings.model,
    }, api: { getLocalAiStatus: () => status({ running: true, state: 'downloading', processed: 3, total: 10 }) } });
    const tree = await ready(h);
    assert.match(text(tree), new RegExp(`Managed by ${owner}`));
    assert.match(text(tree), /LumiaDAM job: running/);
    assert.match(text(tree), /3 \/ 10 MiB/);
    assert.doesNotMatch(text(tree), /3 \/ 10 assets/);
    assert.doesNotMatch(text(tree), /Engine: stopped|Model: not loaded|label suggestions|collections and labels/);
    h.cleanup();
  }
});

test('Özet yanıtı 100 dosya içerse de detay açılmadan DOM oluşmaz; onayın 200 dosyalık tam kapsamı görünür', async () => {
  const items = Array.from({ length: 100 }, (_, i) => ({ id: `asset-${i}`, fileName: `chair-${i}.max`, filePath: `//nas/chair-${i}.max`, libraryName: 'Models' }));
  const suggestion = { id: 'large', kind: 'collection', title: 'Chairs', reason: 'Preview classification',
    assetIds: [], items, totalItems: 200, collectionName: 'Chairs', status: 'pending' };
  const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { api: { listAiSuggestions: () => [suggestion] } });
  let tree = await ready(h);
  assert.equal(reviewItems(tree).length, 0);
  assert.match(text(tree), /Review 200 assets/);
  nodes(tree, n => n.type === 'details')[0].props.onToggle({ currentTarget: { open: true } });
  tree = h.render();
  assert.equal(reviewItems(tree).length, 100);
  assert.match(text(tree), /first 100 of 200 assets/);
  assert.match(text(tree), /Approval includes all 200 assets/);
  assert.doesNotMatch(text(tree), /chair-100\.max/);
  nodes(tree, n => n.type === 'details')[0].props.onToggle({ currentTarget: { open: false } });
  tree = h.render(); assert.equal(reviewItems(tree).length, 0);
  h.cleanup();
});

test('Local AI ana menüden bağımsız çalışma alanına açılır', async () => {
  let opened = 0;
  const h = harness('components/Toolbar/Toolbar.tsx', 'Toolbar');
  const props = { workspace: 'cad', focusedAsset: null, onOpenLocalAi: () => { opened++; } };
  let tree = h.render(props);
  nodes(tree, n => n.props?.['aria-label'] === 'Open LumiaDAM menu')[0].props.onClick();
  tree = h.render(props);
  const entry = nodes(tree, n => n.props?.role === 'menuitem' && text(n) === 'Local AI')[0];
  assert.ok(entry, 'Local AI yalnız ayarlarda değil ana menüde bulunmalı');
  entry.props.onClick();
  assert.equal(opened, 1);
  h.cleanup();
});

test('Genel bakış parti ilerlemesi ile kütüphane kapsamını ayırır ve model kendiliğinden başlamaz', async () => {
  const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { settings: { mode: 'manual', setupComplete: true },
    api: { getLocalAiStatus: () => status({ processed: 20, total: 100, catalogAssets: 1000,
      analyzedAssets: 120, pendingAssets: 680, withoutPreview: 200, skipped: 2 }) } });
  const tree = await ready(h);
  for (const label of ['Overview', 'Model & schedule', 'Suggestions', 'Activity log']) assert.ok(button(tree, label), label);
  assert.match(text(tree), /This batch/);
  assert.match(text(tree), /Waiting for analysis/);
  assert.match(text(tree), /680/);
  assert.match(text(tree), /No preview yet/);
  assert.equal(h.calls.some(([name]) => name === 'startLocalAi'), false);
  h.cleanup();
});

test('Günlük yalnız açıldığında okunur; dosya kullanıcı tarafından açılabilir', async () => {
  const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { api: {
    getLocalAiLog: () => ({ path: 'C:/Local/LumiaDAM/local-ai/activity.jsonl', entries: [
      { at: '2026-10-15T12:00:00Z', event: 'asset_classified', details: { fileName: '9.jpg', category: 'Kitchen' } },
    ] }),
  } });
  let tree = await ready(h);
  assert.equal(h.calls.some(([name]) => name === 'getLocalAiLog'), false);
  assert.ok(button(tree, 'Activity log'), 'Günlük görünümü henüz yok');
  button(tree, 'Activity log').props.onClick();
  tree = h.render(); await settle(); tree = h.render();
  assert.match(text(tree), /9.jpg/);
  assert.match(text(tree), /Kitchen/);
  await button(tree, 'Open log file').props.onClick(); await settle();
  assert.deepEqual(h.calls.find(([name]) => name === 'openLocalAiLog'), ['openLocalAiLog']);
  h.cleanup();
});

test('Yerel AI dili değişince başlık, aşama, kategori ve günlük çevrilir; kimlikler ve kullanıcı dosyaları korunur', async () => {
  const suggestion = { id: 'category:Kitchen:page:0:fixed', kind: 'collection', title: 'Group Kitchen assets across libraries',
    collectionName: 'Kitchen', status: 'pending', reason: 'Suggested from cached previews. Review the files; originals and model references will stay in place.',
    items: [{ id: 'asset', fileName: 'Kitchen.max', filePath: 'C:/Kitchen/Office.max', libraryName: 'Kitchen', basedOn: 'Office.jpg' }] };
  const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { settings: { mode: 'manual', setupComplete: true }, api: {
    getLocalAiStatus: () => status({ phase: 'Classifying cached preview: Kitchen.jpg', catalogAssets: 1234 }),
    listAiSuggestions: () => [suggestion],
    getLocalAiLog: () => ({ path: 'C:/Kitchen/activity.jsonl', entries: [{ at: '2026-10-15T12:00:00Z', event: 'asset_classified', details: {
      fileName: 'Kitchen.max', filePath: 'C:/Kitchen/Office.max', category: 'Kitchen', model: 'qwen3.5:0.8b',
      status: { phase: 'Preparing reviewable collection suggestions', state: 'running' },
    } }] }),
  } });
  let tree = await ready(h);
  assert.ok(button(tree, 'Run now'));
  await h.i18n.setLanguage('tr');
  tree = h.render();
  assert.ok(button(tree, 'Şimdi çalıştır'));
  assert.match(text(tree), /Yerel yapay zekâ düzenleyicisi/);
  assert.match(text(tree), /Önbellekteki önizleme sınıflandırılıyor: Kitchen\.jpg/);
  assert.match(text(tree), /Mutfak/);
  assert.match(text(tree), /1\.234/);
  assert.ok(button(tree, 'Bekleyen tüm önerileri kabul et'));
  assert.equal(input(tree, 'local-ai-model').props.value, 'qwen3.5:0.8b');
  nodes(tree, n => n.type === 'details')[0].props.onToggle({ currentTarget: { open: true } });
  tree = h.render();
  assert.match(text(tree), /C:\/Kitchen\/Office\.max/);
  assert.match(text(tree), /\(Kitchen\)/);
  assert.match(text(tree), /Office\.jpg dosyasına bağlandı/);
  button(tree, 'İşlem günlüğü').props.onClick();
  h.render(); await settle(); tree = h.render();
  assert.match(text(tree), /Dosya sınıflandırıldı/);
  assert.match(text(tree), /İncelenebilir koleksiyon önerileri hazırlanıyor/);
  const rawDetails = JSON.parse(text(nodes(tree, n => n.type === 'pre')[0]));
  assert.equal(rawDetails.category, 'Mutfak');
  assert.equal(rawDetails.fileName, 'Kitchen.max');
  assert.equal(rawDetails.model, 'qwen3.5:0.8b');
  assert.equal(rawDetails.filePath, 'C:/Kitchen/Office.max');
  await button(tree, 'Koleksiyonu onayla').props.onClick(); await settle();
  assert.deepEqual(h.calls.find(([name]) => name === 'applyAiSuggestion'), ['applyAiSuggestion', suggestion.id]);
  await h.i18n.setLanguage('en');
  assert.ok(button(h.render(), 'Run now'));
  h.cleanup();
});

test('Türkçe AI doğrulama, model indirme, motor hatası ve yinelenen önizleme gerekçesi görünür', async () => {
  const h = harness('components/LocalAISettings.tsx', 'LocalAISettings', { language: 'tr', settings: { mode: 'manual', setupComplete: true }, api: {
    getLocalAiStatus: () => status({ phase: 'Model download server returned HTTP 404 Not Found.', error: 'This local model does not support images.' }),
    listAiSuggestions: () => [{ id: 'preview:sample', kind: 'duplicates', title: 'Review matching previews', collectionName: 'Review: matching previews',
      reason: 'Matching thumbnail fingerprints are candidates, not proof of identical originals. Approval creates a review collection; nothing is deleted.', status: 'pending', items: [] }],
  } });
  let tree = await ready(h);
  assert.match(text(tree), /Model indirme sunucusu HTTP 404 Not Found döndürdü/);
  assert.match(text(tree), /Bu yerel model görüntüleri desteklemiyor/);
  assert.match(text(tree), /İnceleme: eşleşen önizlemeler/);
  assert.match(text(tree), /hiçbir dosya silinmez/);
  await button(tree, 'Modeli indir').props.onClick(); await settle();
  assert.match(text(h.render()), /Model indirme başladı/);
  input(h.render(), 'local-ai-batch').props.onChange({ target: { value: '0' } });
  tree = h.render();
  assert.equal(button(tree, 'Ayarları kaydet').props.disabled, true);
  assert.match(text(tree), /Parti boyutu 1 ile 1\.000 dosya arasında olmalıdır/);
  h.cleanup();
});
