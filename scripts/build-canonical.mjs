#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const yarn = process.platform === 'win32' ? 'yarn.cmd' : 'yarn';

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: 'inherit',
  });
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(
      `${command} ${args.join(' ')} failed with ${result.status}`,
    );
}

async function sha256(file) {
  return createHash('sha256')
    .update(await readFile(file))
    .digest('hex');
}

run(yarn, ['workspace', 'plec', 'build:wasm']);
run(process.execPath, ['packages/plec/scripts/build-artifact.mjs']);

const runtime = path.join(root, 'packages/plec/dist/runtime');
const provenance = JSON.parse(
  await readFile(path.join(runtime, 'provenance.json'), 'utf8'),
);
const packageWasmHash = await sha256(
  path.join(runtime, 'runtime_bg.wasm'),
);
if (packageWasmHash !== provenance.wasmSha256)
  throw new Error('release runtime hash differs from provenance.json');

run(yarn, ['workspace', 'fullstack', 'build']);

const staged = path.join(root, 'apps/fullstack/dist/public/runtime');
const stagedWasmHash = await sha256(
  path.join(staged, 'runtime_bg.wasm'),
);
const stagedJsHash = await sha256(path.join(staged, 'runtime.js'));
if (
  stagedWasmHash !== packageWasmHash ||
  stagedJsHash !== provenance.jsSha256
)
  throw new Error(
    'fullstack staged runtime differs from installed plec package',
  );

console.log(`Canonical runtime verified: ${packageWasmHash}`);
run(yarn, ['test:e2e']);
