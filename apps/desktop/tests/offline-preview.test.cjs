const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');
const ts = require('typescript');
const React = require('react');
const { renderToStaticMarkup } = require('react-dom/server');

function render(extension, thumbnailPath) {
  const asset = { id: 'image', extension, kind: extension === 'mp4' ? 'video' : 'image', filePath: `//sleeping-nas/share/image.${extension}`,
    fileName: `image.${extension}`, thumbnailPath, metadata: {}, tags: [], collections: [], colorLabel: 'none', sizeBytes: 1, rating: 0 };
  const exports = {};
  const store = selector => selector({ focusedAssetId: asset.id, assets: [asset], tags: [], collections: [] });
  const source = readFileSync(join(__dirname, '../src/components/DetailPanel/DetailPanel.tsx'), 'utf8');
  runInNewContext(ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022 } }).outputText, {
    exports, require: path => {
      if (path === 'react' || path === 'react/jsx-runtime') return require(path);
      if (path === 'clsx') return { default: require(path) };
      if (path.endsWith('/store')) return { useUIStore: store, useAssetStore: store, useTagStore: store, useCollectionStore: store };
      if (path === '@tauri-apps/api/core') return { convertFileSrc: value => value };
      if (path.endsWith('.css')) return { default: {} };
      return new Proxy({}, { get: () => () => null });
    },
  });
  return renderToStaticMarkup(React.createElement(exports.DetailPanel));
}

test('Ayrıntı önizlemesi NAS kaynağını okumaz; yalnız yerel cache kullanır', () => {
  for (const extension of ['jpg', 'png', 'exr', 'hdr', 'mp4']) {
    const html = render(extension, 'C:/cache/asset/preview.png');
    assert.match(html, /src="C:\/cache\/asset\/preview.png"/);
    assert.doesNotMatch(html, /src="\/\/sleeping-nas/);
    assert.doesNotMatch(render(extension), /src="\/\/sleeping-nas/);
  }
});
