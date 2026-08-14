import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { compileRouteEntry, readSourceGraph } from './node-entry.ts';

describe('readSourceGraph', () => {
  it('follows authored JavaScript specifiers to available TypeScript source', async () => {
    const repoRoot = process.cwd();
    const entry = path.join(
      repoRoot,
      'packages/lucide-plec/src/icons/workflow.ts',
    );
    const modules = await readSourceGraph(entry, repoRoot, repoRoot);

    expect(modules.map((module) => module.filePath)).toContain(
      path.join(repoRoot, 'packages/lucide-plec/src/create-icon.ts'),
    );
  });
});

describe('Slice 3 route cutover', () => {
  it('passes the removed legacy-renderer guard before resolving the entry', async () => {
    await expect(
      compileRouteEntry('unused.tsx', {
        rootDir: '.',
        repoRootDir: '.',
      }),
    ).rejects.toThrow(/ENOENT/);
  });
});
