import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { expect, test } from '@playwright/test';

const publicDir = path.resolve(
  import.meta.dirname,
  '../../../..',
  'apps/fullstack/dist/public',
);

test('compiled route artifacts are typed graphs', async () => {
  const manifest = JSON.parse(
    await readFile(path.join(publicDir, 'route-manifest.json'), 'utf8'),
  );
  const todos = manifest.routes.find((route) => route.path === 'todos');
  const stress = manifest.routes.find(
    (route) => route.path === 'stress',
  );
  expect(
    Number.isInteger(todos?.loaderAction),
    'todos needs a typed loader action',
  ).toBe(true);
  expect(
    stress?.graphId,
    'stress needs a compiled route graph',
  ).toBeTruthy();

  const graphIds = new Set([
    manifest.rootGraphId,
    ...manifest.routes.flatMap((route) =>
      [route.graphId, route.pendingGraphId, route.errorGraphId].filter(
        Boolean,
      ),
    ),
  ]);
  for (const id of graphIds) {
    const graphFile = id.replace(/[\/\\]/g, '--').replace('#', '--');
    const graph = JSON.parse(
      await readFile(
        path.join(publicDir, 'graphs', `${graphFile}.json`),
        'utf8',
      ),
    );
    expect(graph.version, `${id} is not a typed graph`).toBe('0.10');
  }

  const stressGraphFile = stress.graphId
    .replace(/[\/\\]/g, '--')
    .replace('#', '--');
  const stressGraph = JSON.parse(
    await readFile(
      path.join(publicDir, 'graphs', `${stressGraphFile}.json`),
      'utf8',
    ),
  );
  const stressComponent =
    stressGraph.components[stressGraph.rootComponent];
  expect(
    stressComponent.inputs.map(
      (input: { name: string }) => stressComponent.strings[input.name],
    ),
  ).toEqual(['instruments', 'summary', 'events']);
  expect(
    stressComponent.loops.length,
    'stress needs keyed grid, summary, telemetry, and event loops',
  ).toBeGreaterThanOrEqual(4);

  console.log(`[plec-e2e] ${graphIds.size} typed graphs verified`);
});
