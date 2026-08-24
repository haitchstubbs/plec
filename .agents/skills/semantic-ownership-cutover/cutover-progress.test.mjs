import assert from 'node:assert/strict';
import test from 'node:test';
import { evaluate, summary, validateManifest } from './cutover-progress.mjs';

function manifest() {
  return { frozenDenominator: 10, contracts: [
    { id: 'base', weight: 4, stages: ['source', 'HIR', 'IR', 'runtime', 'browser'], prerequisites: [], probes: [{ command: 'pass', args: [], expect: 'ok' }] },
    { id: 'next', weight: 6, stages: ['source', 'HIR', 'IR', 'runtime', 'browser'], prerequisites: ['base'], probes: [{ command: 'next', args: [], expect: 'ok' }] },
  ] };
}

test('rejects invalid manifest contracts', () => {
  for (const mutate of [(value) => { value.contracts[1].id = 'base'; }, (value) => { value.contracts[0].weight = 0; }, (value) => { value.contracts[1].prerequisites = ['missing']; }, (value) => { value.contracts[0].probes = []; }]) {
    const value = manifest(); mutate(value); assert.throws(() => validateManifest(value));
  }
});

test('reports ownership, blocked contracts, and target delta', () => {
  const value = manifest();
  const allPassed = evaluate(value, { run: () => true });
  assert.equal(allPassed.get('base').status, 'owned');
  assert.equal(allPassed.get('next').status, 'owned');
  let results = evaluate(value, { run: (probe) => probe.command === 'pass' });
  assert.equal(results.get('base').status, 'owned');
  assert.equal(results.get('next').status, 'failed');
  assert.deepEqual(summary(value, results, ['next']), { owned: 4, remaining: 6, plannedDelta: 6, postSliceRemaining: 0 });
  value.contracts[0].probes[0].command = 'fail';
  results = evaluate(value, { run: () => false });
  assert.equal(results.get('base').status, 'failed');
  assert.equal(results.get('next').status, 'blocked');
});
