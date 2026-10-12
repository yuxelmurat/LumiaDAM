const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');
const ts = require('typescript');

test('Yalnız görünür kartlar ve kardeş görseller tek toplu istekle önceliklendirilir', async () => {
  const exports = {}, requests = [], timers = new Map();
  let observer, timerId = 0;
  const element = (id, key = id) => ({ getAttribute: (name) => name === 'data-preview-id' ? id : key });
  const visible = element('jpg'), related = element('companion'), hidden = element('hidden');
  const source = readFileSync(join(__dirname, '../src/components/AssetGrid/visiblePreviews.ts'), 'utf8');
  const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  runInNewContext(compiled, { exports,
    IntersectionObserver: class { constructor(callback) { this.callback = callback; observer = this; } observe() {} disconnect() {} },
    setTimeout: (fn) => { timers.set(++timerId, fn); return timerId; }, clearTimeout: id => timers.delete(id),
  });
  const stop = exports.observeVisiblePreviews({ querySelectorAll: () => [visible, related, hidden] }, new Set(), async ids => requests.push(Array.from(ids)));
  observer.callback([{ target: visible, isIntersecting: true }, { target: related, isIntersecting: true }, { target: hidden, isIntersecting: false }]);
  observer.callback([{ target: visible, isIntersecting: true }]);
  assert.equal(timers.size, 1);
  [...timers.values()][0](); await new Promise(setImmediate);
  assert.deepEqual(requests, [['jpg', 'companion']]);
  observer.callback([{ target: visible, isIntersecting: true }]);
  assert.equal(requests.length, 1);
  stop();
});
