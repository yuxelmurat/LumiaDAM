const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');
const ts = require('typescript');

test('Editörün dilini değiştirmek iframe adresini, anahtarını veya açılmış belgesini değiştirmez', async () => {
  const hooks = [], effects = [], messages = [], calls = [], listeners = new Map();
  let cursor = 0, language = 'en';
  const target = { postMessage: data => messages.push(data) };
  const react = {
    forwardRef: component => component,
    useImperativeHandle() {},
    useRef: initial => hooks[cursor++] ??= { current: initial },
    useState: initial => {
      const index = cursor++;
      hooks[index] ??= { value: initial };
      return [hooks[index].value, value => { hooks[index].value = typeof value === 'function' ? value(hooks[index].value) : value; }];
    },
    useEffect: (effect, deps) => {
      const index = cursor++;
      if (!hooks[index] || deps.some((value, i) => !Object.is(hooks[index].deps[i], value))) {
        hooks[index]?.cleanup?.(); hooks[index] = { deps };
        effects.push(() => { hooks[index].cleanup = effect(); });
      }
    },
  };
  const exports = {};
  const source = readFileSync(join(__dirname, '../src/components/Editor/EditorPanel.tsx'), 'utf8');
  runInNewContext(ts.transpileModule(source, { compilerOptions: {
    module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.ReactJSX,
  } }).outputText, {
    exports, console, ArrayBuffer, clearTimeout() {}, location: { origin: 'http://tauri.localhost' },
    window: { setTimeout: () => 1, addEventListener: (name, fn) => listeners.set(name, fn), removeEventListener: name => listeners.delete(name) },
    require: name => {
      if (name === 'react') return react;
      if (name === 'react/jsx-runtime') return require(name);
      if (name.endsWith('/i18n')) return { useTranslation: () => ({ language, t: key => key }), translateMessage: text => text };
      if (name === './editors') return { editors: { photo: { engine: 'PhotoCraft', path: '/photocraft/index.html', channel: 'lumiadam-editor-v1' } } };
      if (name === '@tauri-apps/api/core') return { invoke: async name => { calls.push(name); return new ArrayBuffer(1); } };
      if (name === '@tauri-apps/api/window') return { getCurrentWindow: () => ({ onCloseRequested: async () => () => {} }) };
      if (name.endsWith('.css')) return { default: {} };
      return { confirm: async () => false };
    },
  });
  const render = () => {
    cursor = 0;
    const tree = exports.EditorPanel({ asset: { id: 'asset', fileName: 'Save.jpg' }, editor: 'photo' }, {});
    const iframe = tree.props.children.find(node => node?.type === 'iframe');
    iframe.ref.current = { contentWindow: target };
    effects.splice(0).forEach(effect => effect());
    return iframe;
  };
  const initial = render();
  await listeners.get('message')({ source: target, origin: 'http://tauri.localhost', data: { channel: 'lumiadam-editor-v1', type: 'ready' } });
  language = 'tr'; const turkish = render();
  assert.equal(turkish.props.src, initial.props.src);
  assert.equal(turkish.key, initial.key);
  assert.equal(messages.at(-1).language, 'tr');
  await listeners.get('message')({ source: target, origin: 'http://tauri.localhost', data: { channel: 'lumiadam-editor-v1', type: 'ready' } });
  assert.deepEqual(calls, ['read_editor_asset']);
  assert.equal(messages.filter(message => message.type === 'open').length, 1);
  language = 'en'; const english = render();
  assert.equal(english.props.src, initial.props.src);
  assert.equal(messages.at(-1).language, 'en');
  hooks.forEach(hook => hook?.cleanup?.());
});

test('Türkçe native katalog yer tutucularını ve tek çoğul formunu korur', async () => {
  const { validateTurkishCatalog } = await import('../../../integrations/craft-editors/localization.mjs');
  const rows = validateTurkishCatalog(readFileSync(join(__dirname, '../../../integrations/craft-editors/tr.tsv'), 'utf8'));
  assert.ok(rows.length > 2500);
  assert.ok(rows.some(([context, source, translation]) => context === '@plural' && source === '{n} Layer|{n} Layers' && translation === '{n} katman'));
  assert.doesNotThrow(() => validateTurkishCatalog('\tOpened {name} with {count} layers\t{count} katmanla {name} açıldı\n'));
  assert.throws(() => validateTurkishCatalog('\tSaved {path}\tKaydedildi\n'), /yer tutucu/);
  assert.throws(() => validateTurkishCatalog('\tSave\tKaydet\n\tSave\tKayıt\n'), /Yinelenen/);
  assert.throws(() => validateTurkishCatalog('@plural\t{n} file|{n} files\t{n} dosya|{n} dosyalar\n'), /tek form/);
});
