import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { compileRouteEntry, readSourceGraph, validateRouteLoaderAction } from './node-entry.ts';
import { compileComponentGraph } from './index.ts';

describe('readSourceGraph', () => {
  it('follows authored JavaScript specifiers to available TypeScript source', async () => {
    const repoRoot = path.resolve(
      path.dirname(fileURLToPath(import.meta.url)),
      '../../..',
    );
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
  it('requires one valid terminal fetch for typed route loaders', () => {
    expect(() => validateRouteLoaderAction({ instructions: [] }, 'routes/todos.tsx')).toThrow(
      /exactly one terminal fetch/,
    );
    expect(() =>
      validateRouteLoaderAction({
        instructions: [
          { op: 'capabilityRequest', capability: 'fetch', successPc: 1, failurePc: 1 },
          { op: 'return' },
        ],
      }),
    ).not.toThrow();
    expect(() =>
      validateRouteLoaderAction({
        instructions: [
          { op: 'capabilityRequest', capability: 'fetch', successPc: 1, failurePc: 1 },
          { op: 'capabilityRequest', capability: 'fetch', successPc: 2, failurePc: 2 },
          { op: 'return' },
        ],
      }),
    ).toThrow(/exactly one terminal fetch/);
    expect(() =>
      validateRouteLoaderAction({
        parameterSlots: [1],
        instructions: [
          { op: 'capabilityRequest', capability: 'fetch', successPc: 1, failurePc: 1 },
          { op: 'return' },
        ],
      }),
    ).toThrow(/invalid loader context slots/);
    expect(() => validateRouteLoaderAction({
      instructions: [
        { op: 'storeState' },
        { op: 'capabilityRequest', capability: 'fetch', successPc: 2, failurePc: 2 },
        { op: 'return' },
      ],
    })).toThrow(/loaderResultState instead of storeState/);
  });

  it('records the supported loader-data state without emitting an executable loader read', () => {
    const result = compileComponentGraph(
      `import { useState } from 'plec';
       const Route = {} as any;
       export function Page() {
         const loaded = Route.useLoaderData();
         const [items, setItems] = useState(loaded);
         return <div>{items}</div>;
       }`,
      { rootComponent: 'Page', mode: 'strict' },
    );
    expect(result.loaderResultState).toBe(0);
    expect(result.graph.constants).toContain(null);
  });
});
