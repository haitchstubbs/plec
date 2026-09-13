import fs from 'node:fs';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * The release artifact (packages/plec with bin/ + dist/ assembled by
 * scripts/build-artifact.mjs) must be consumable as a folder alone. These
 * checks run against the assembled output and skip when it has not been
 * built yet.
 */
const packageDir = path.resolve(import.meta.dirname, '..');
const distDir = path.join(packageDir, 'dist');
const built = existsSync(path.join(distDir, 'index.js'));

describe.skipIf(!built)('plec release artifact', () => {
  const isWindows = process.platform === 'win32';
  const binaryName = isWindows ? 'plec.exe' : 'plec';

  it('stages the WASM runtime assets with brotli sidecars', () => {
    expect(
      existsSync(path.join(distDir, 'runtime/runtime_bg.wasm')),
    ).toBe(true);
    expect(
      existsSync(path.join(distDir, 'runtime/runtime_bg.wasm.br')),
    ).toBe(true);
    expect(existsSync(path.join(distDir, 'runtime/runtime.js'))).toBe(
      true,
    );
    expect(
      existsSync(path.join(distDir, 'runtime/runtime.js.br')),
    ).toBe(true);
  });

  it('carries the release CLI binary the bin shim needs', () => {
    expect(existsSync(path.join(distDir, 'bin', binaryName))).toBe(
      true,
    );
  });

  it('exposes self-contained server and browser entries', () => {
    expect(existsSync(path.join(distDir, 'server.js'))).toBe(true);
    expect(existsSync(path.join(distDir, 'browser.js'))).toBe(true);
    expect(existsSync(path.join(distDir, 'server.d.ts'))).toBe(true);
    expect(existsSync(path.join(distDir, 'browser.d.ts'))).toBe(true);
    expect(existsSync(path.join(distDir, 'node-runtime.mjs'))).toBe(
      true,
    );
  });

  it('never references the monorepo from shipped JS', () => {
    const offenders: string[] = [];
    const walk = (directory: string) => {
      for (const item of fs.readdirSync(directory, {
        withFileTypes: true,
      })) {
        const entry = path.join(directory, item.name);
        if (item.isDirectory()) {
          walk(entry);
          continue;
        }
        if (!/\.(js|mjs|cjs)$/.test(item.name)) continue;
        // Test files quote the audited patterns themselves.
        if (item.name.includes('.test.')) continue;
        const source = fs.readFileSync(entry, 'utf8');
        if (
          /(?:from|import)\s*['"]plec-browser['"]/.test(source) ||
          source.includes('workspace:')
        )
          offenders.push(path.relative(distDir, entry));
      }
    };
    walk(distDir);
    expect(offenders).toEqual([]);
  });

  it('keeps the browser entry free of Node builtins', () => {
    const source = fs.readFileSync(
      path.join(distDir, 'browser.js'),
      'utf8',
    );
    expect(/(?:from|import)\s*['"]node:/.test(source)).toBe(false);
  });
});
