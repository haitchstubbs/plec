import test from 'node:test';
import assert from 'node:assert/strict';
import { plec, plecHtmlFinalizer } from './index.js';

test('Plec Vite adapter provides Vite client and stylesheet to SSR documents', () => {
  const plugin = plec({
    root: process.cwd(),
    source: 'src/router.tsx',
    outDir: 'dist',
    internalPort: 3001,
    cli: 'plec',
  });
  const pre = plugin.transformIndexHtml.handler(
    '<html><head><link rel="stylesheet" href="/assets/styles.css"></head><body><script type="module" src="/_plec/assets/client.js?v=abc"></script></body></html>',
  );
  assert.doesNotMatch(pre, /<script type="module" src="\/_plec\//);
  const html = plecHtmlFinalizer().transformIndexHtml.handler(
    pre.replace(
      '</head>',
      '<script type="module" src="/@vite/client"></script></head>',
    ),
  );
  assert.match(html, /href="\/src\/styles\.css"/);
  assert.match(html, /src="\/@vite\/client"/);
  assert.match(html, /src="\/_plec\/assets\/client\.js\?v=abc"/);
});
