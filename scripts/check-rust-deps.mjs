#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { cargoToolPath, repoRoot } from './toolchain.mjs';

const full = process.argv.includes('--full');
const unknownArgs = process.argv
  .slice(2)
  .filter((arg) => arg !== '--full');
if (unknownArgs.length) {
  console.error(`Unknown argument(s): ${unknownArgs.join(' ')}`);
  console.error('Usage: node scripts/check-rust-deps.mjs [--full]');
  process.exit(2);
}

function run(label, command, args, options = {}) {
  console.log(`\n==> ${label}`);
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    stdio: 'inherit',
    ...options,
    shell: false,
  });
  if (result.error) {
    console.error(`${label} failed: ${result.error.message}`);
    process.exit(result.status ?? 1);
  }
  if (result.status !== 0) process.exit(result.status ?? 1);
}

run(
  'Validate locked Cargo resolution',
  'cargo',
  ['metadata', '--locked', '--format-version', '1'],
  { stdio: ['inherit', 'ignore', 'inherit'] },
);

const deny = cargoToolPath('cargo-deny');
const audit = cargoToolPath('cargo-audit');
run('Check Cargo dependency policy', deny, [
  'check',
  '--disable-fetch',
  '--hide-inclusion-graph',
  'licenses',
  'bans',
  'sources',
]);

if (full) {
  run('Check RustSec advisories with cargo-deny', deny, [
    'check',
    '--hide-inclusion-graph',
    'advisories',
  ]);
  run('Audit RustSec advisories with cargo-audit', audit, ['audit']);
} else {
  console.log(
    '\nFast dependency policy checks passed. Use --full for RustSec advisory checks.',
  );
}
