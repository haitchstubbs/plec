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
      'packages/lucide-plec/src/index.ts',
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

  it('lowers the router-owned error retry callback', () => {
    const result = compileComponentGraph(
      `export function ErrorView({ retry }: { retry?: () => void }) { return <button onClick={retry}>Retry</button>; }`,
      { rootComponent: 'ErrorView', mode: 'strict', routeRetryProp: 'retry' },
    );
    expect(result.diagnostics).toEqual([]);
    expect(result.graph.actions[0]).toMatchObject({ routeRetry: true });
  });

  it('gives error components a named failure state without claiming ordinary actions', () => {
    const result = compileComponentGraph(
      `import { useState } from 'plec';
       export function ErrorView({ error, retry }: { error: { message: string }; retry?: () => void }) {
         const [count, setCount] = useState(0);
         return <div>{error.message}<button onClick={retry}>Retry</button><button onClick={() => setCount(count + 1)}>{count}</button></div>;
       }`,
      { rootComponent: 'ErrorView', mode: 'strict', routeRetryProp: 'retry', routeErrorProp: 'error' },
    );
    expect(result.graph.routeErrorState).toBe(1);
    expect(result.graph.strings[result.graph.stateSlots[1]!.name!]).toBe('error');
    expect(result.graph.expressions.some((program) => program.instructions.some(
      (instruction) => instruction.op === 'loadState' && instruction.state === 1,
    ))).toBe(true);
    expect(result.graph.actions.filter((action) => action.routeRetry)).toHaveLength(1);
    expect(result.graph.actions.some((action) => !action.routeRetry)).toBe(true);
  });
});
