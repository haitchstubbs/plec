import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const stages = new Set(['source', 'HIR', 'IR', 'runtime', 'browser']);

export function validateManifest(manifest) {
  if (!Number.isFinite(manifest.frozenDenominator) || manifest.frozenDenominator <= 0) throw new Error('frozenDenominator must be positive');
  if (!Array.isArray(manifest.contracts) || manifest.contracts.length === 0) throw new Error('contracts must not be empty');
  const ids = new Set();
  let weight = 0;
  for (const contract of manifest.contracts) {
    if (!/^[a-z0-9-]+$/.test(contract.id ?? '')) throw new Error(`invalid contract id: ${contract.id}`);
    if (ids.has(contract.id)) throw new Error(`duplicate contract id: ${contract.id}`);
    ids.add(contract.id);
    if (!Number.isFinite(contract.weight) || contract.weight <= 0) throw new Error(`contract ${contract.id} has non-positive weight`);
    weight += contract.weight;
    if (!Array.isArray(contract.stages) || contract.stages.length !== stages.size || !contract.stages.every((stage) => stages.has(stage))) throw new Error(`contract ${contract.id} must require source, HIR, IR, runtime, and browser`);
    if (!Array.isArray(contract.probes) || contract.probes.length === 0) throw new Error(`contract ${contract.id} has no probes`);
    for (const probe of contract.probes) if (!probe.command || !Array.isArray(probe.args) || !probe.expect) throw new Error(`contract ${contract.id} has an invalid probe`);
  }
  if (weight !== manifest.frozenDenominator) throw new Error(`contract weights ${weight} do not equal frozenDenominator ${manifest.frozenDenominator}`);
  for (const contract of manifest.contracts) for (const prerequisite of contract.prerequisites ?? []) if (!ids.has(prerequisite)) throw new Error(`contract ${contract.id} has unknown prerequisite ${prerequisite}`);
}

function runProbe(probe, cwd) {
  const result = spawnSync(probe.command, probe.args, { cwd, encoding: 'utf8' });
  return result.status === 0 && `${result.stdout ?? ''}${result.stderr ?? ''}`.includes(probe.expect);
}

export function evaluate(manifest, { cwd, run = runProbe } = {}) {
  validateManifest(manifest);
  const results = new Map();
  for (const contract of manifest.contracts) {
    if ((contract.prerequisites ?? []).some((id) => results.get(id)?.status !== 'owned')) results.set(contract.id, { contract, status: 'blocked' });
    else results.set(contract.id, { contract, status: contract.probes.every((probe) => run(probe, cwd)) ? 'owned' : 'failed' });
  }
  return results;
}

export function summary(manifest, results, targets = []) {
  const byId = new Map(manifest.contracts.map((contract) => [contract.id, contract]));
  for (const target of targets) if (!byId.has(target)) throw new Error(`unknown target ${target}`);
  const owned = [...results.values()].filter(({ status }) => status === 'owned').reduce((total, { contract }) => total + contract.weight, 0);
  const plannedDelta = targets.reduce((total, id) => total + (results.get(id)?.status === 'owned' ? 0 : byId.get(id).weight), 0);
  return { owned, remaining: manifest.frozenDenominator - owned, plannedDelta, postSliceRemaining: manifest.frozenDenominator - owned - plannedDelta };
}

function main() {
  const skillDir = path.dirname(fileURLToPath(import.meta.url));
  const root = path.resolve(skillDir, '../../..');
  const targets = process.argv[2] === '--targets' && process.argv.length === 4 ? process.argv[3].split(',').filter(Boolean) : process.argv.length === 2 ? [] : (() => { throw new Error('usage: node cutover-progress.mjs [--targets id,id]'); })();
  const manifest = JSON.parse(readFileSync(path.join(skillDir, 'cutover-contracts.json'), 'utf8'));
  const results = evaluate(manifest, { cwd: root });
  const score = summary(manifest, results, targets);
  const percent = (value) => `${((value / manifest.frozenDenominator) * 100).toFixed(1)}%`;
  console.log(`Rust-owned: ${score.owned}/${manifest.frozenDenominator} (${percent(score.owned)})`);
  console.log(`Cutover remaining: ${score.remaining}/${manifest.frozenDenominator} (${percent(score.remaining)})`);
  for (const { contract, status } of results.values()) console.log(`${status.toUpperCase()} ${contract.id} (${contract.weight})`);
  if (targets.length) {
    console.log(`Planned delta: ${score.plannedDelta} points (${percent(score.plannedDelta)})`);
    console.log(`Post-slice remaining: ${score.postSliceRemaining}/${manifest.frozenDenominator} (${percent(score.postSliceRemaining)})`);
  }
  process.exit([...results.values()].some(({ status }) => status === 'failed') ? 1 : 0);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
