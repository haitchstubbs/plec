#!/usr/bin/env node
/**
 * `plec` bin shim: the `plec` command is owned by this package, so apps do
 * not need a global cargo install. `serve` runs @plec/node directly;
 * compiler/inspection commands use the native CLI resolution order:
 *
 *   1. PLEC_BIN env override (authoritative: a set-but-unusable value is an
 *      error, never a silent fallback)
 *   2. packaged binary `dist/bin/plec[.exe]` assembled by
 *      scripts/build-artifact.mjs (release variant)
 *   3. `~/.cargo/bin/plec[.exe]` (cargo-installed binary; the dev variant
 *      installed by `yarn install:plec-cli:dev` also carries the `workspace`
 *      command group)
 *
 * Node-only and dependency-free (node builtins): package.json#bin never
 * enters a browser bundle.
 */
import { spawn, spawnSync } from 'node:child_process';
import { accessSync, constants, existsSync } from 'node:fs';
import { parseServeOptions, SERVE_HELP } from './serve-options.js';
import os from 'node:os';
import path from 'node:path';

const binaryName = process.platform === 'win32' ? 'plec.exe' : 'plec';
// bin/ -> package root -> packaged release CLI.
const packagedBinary = path.join(
  import.meta.dirname,
  '..',
  'dist',
  'bin',
  binaryName,
);
const cargoBinary = path.join(
  os.homedir(),
  '.cargo',
  'bin',
  binaryName,
);
const workspaceCliManifest = path.join(
  import.meta.dirname,
  '..',
  '..',
  '..',
  'crates',
  'plec-cli',
  'Cargo.toml',
);

const [command, ...args] = process.argv.slice(2);
if (command === 'serve') {
  try {
    const parsed = parseServeOptions(args);
    if (parsed.help) {
      console.log(SERVE_HELP);
    } else {
      const { serve } = await import('@plec/node');
      await serve(parsed.options);
    }
  } catch (error) {
    console.error(
      `plec serve: ${error instanceof Error ? error.message : String(error)}`,
    );
    process.exitCode = 1;
  }
} else if (command === 'dev' && existsSync(workspaceCliManifest)) {
  // A globally installed dev CLI may predate the checked-out source. Build
  // and run this workspace's coordinator so its child build commands use the
  // same current compiler/build implementation.
  const build = spawnSync(
    process.env.CARGO || 'cargo',
    ['build', '--manifest-path', workspaceCliManifest],
    { stdio: 'inherit' },
  );
  if (build.error) {
    console.error(
      `plec dev: failed to build workspace CLI: ${build.error.message}`,
    );
    process.exit(1);
  }
  if (build.status !== 0) process.exit(build.status ?? 1);
  const workspaceRoot = path.resolve(
    path.dirname(workspaceCliManifest),
    '..',
    '..',
  );
  const workspaceBinary = path.join(
    workspaceRoot,
    'target',
    'debug',
    binaryName,
  );
  const child = spawn(workspaceBinary, [command, ...args], {
    stdio: 'inherit',
  });
  for (const signal of ['SIGINT', 'SIGTERM'])
    process.once(signal, () => child.kill(signal));
  child.on('error', (error) => {
    console.error(
      `plec dev: failed to run workspace CLI: ${error.message}`,
    );
    process.exit(1);
  });
  child.on('exit', (code, signal) => {
    if (signal) process.kill(process.pid, signal);
    else process.exit(code ?? 0);
  });
} else {
  const { binary } = resolveBinary();

  const child = spawn(binary, process.argv.slice(2), {
    stdio: 'inherit',
  });
  child.on('error', (error) => {
    console.error(`plec: failed to run ${binary}: ${error.message}`);
    process.exit(1);
  });
  child.on('exit', (code, signal) => {
    if (signal) process.kill(process.pid, signal);
    else process.exit(code ?? 0);
  });
}

function usable(candidate) {
  try {
    accessSync(candidate, constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

function resolveBinary() {
  const override = process.env.PLEC_BIN;
  if (override) {
    const resolved = path.resolve(override);
    if (usable(resolved)) return { binary: resolved };
    fail([{ label: `PLEC_BIN (env override)`, path: resolved }]);
  }
  if (usable(packagedBinary)) return { binary: packagedBinary };
  if (usable(cargoBinary)) return { binary: cargoBinary };
  fail([
    { label: 'packaged release CLI', path: packagedBinary },
    { label: 'cargo-installed CLI', path: cargoBinary },
  ]);
}

function fail(searched) {
  console.error('plec: could not find an executable plec CLI binary.');
  console.error('plec: searched:');
  for (const { label, path: candidate } of searched)
    console.error(`plec:   - ${candidate} (${label})`);
  console.error(
    'plec: build the CLI from source: `yarn workspace @plec/core build:artifact` copies the release binary into the artifact (PLEC_CLI_VERSION in .env.plec selects the variant).',
  );
  console.error(
    'plec: or install it globally: `yarn install:plec-cli:dev` (dev variant, includes the `workspace` command group).',
  );
  process.exit(1);
}
