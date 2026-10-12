const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');
const ts = require('typescript');
function load() {
  const exports = {};
  const source = readFileSync(join(__dirname, '../src/components/Editor/editors.ts'), 'utf8');
  runInNewContext(ts.transpileModule(source, { compilerOptions: {module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022} }).outputText,{exports});
  return exports;
}
test('PDF ve vektör projeleri doğru editöre yönlenir; görseller PhotoEditor ile açılır', () => {
  const { editorForAsset, canOpenInEditor } = load();
  for (const [extension, expected] of [['PDF','pdf'],['svg','vector'],['svgz','vector'],['AI','vector'],['vectorcraft','vector'],['drawcraft','vector'],['jpg','photo'],['pcraft','photo']])
    assert.equal(editorForAsset({extension}),expected);
  for (const extension of ['max','zip','exe']) assert.equal(editorForAsset({extension}),null);
  assert.equal(canOpenInEditor({extension:'jpg'},'vector'),true);
  assert.equal(canOpenInEditor({extension:'pdf'},'photo'),false);
  assert.equal(canOpenInEditor({extension:'svg'},'pdf'),false);
  assert.equal(canOpenInEditor(null,'pdf'),false);
});
