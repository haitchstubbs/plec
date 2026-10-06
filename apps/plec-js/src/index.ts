#!/usr/bin/env node

import { spawn } from 'node:child_process';
import { accessSync, constants, type PathLike } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

type SearchedCandidate = Readonly<{
  label: string;
  path: PathLike;
}>;

type ResolvedBinary = Readonly<{
  binary: string;
}>;

enum PlecExecutable {
  Default = 'plec',
  Windows = 'plec.exe',
}

enum Platform {
  Windows = 'win32',
}

enum BinaryPath {
  Packaged = '../dist/bin',
  Cargo = '.cargo/bin',
}

enum Log {
  EnvOverride = 'PLEC_BIN (env override)',
  PackagedCLI = 'packaged release CLI',
  CompiledCLI = 'cargo-installed CLI',
  NoExecutableFound = 'plec: could not find an executable plec CLI binary.',
  Searched = 'plec: searched:',
  BuildRequired = 'plec: build the CLI from source: `yarn workspace @plec/core build:artifact` copies the release binary into the artifact (PLEC_CLI_VERSION in .env.plec selects the variant).',
  InstallRequired = 'plec: or install it globally: `yarn install:plec-cli:dev` (dev variant, includes the `workspace` command group).',
}

function check(operation: () => void): boolean {
  try {
    operation();
    return true;
  } catch {
    return false;
  }
}

function usable(candidate: PathLike): boolean {
  return check(() => accessSync(candidate, constants.X_OK));
}

function failure(searched: readonly SearchedCandidate[]): never {
  const searchedCandidates = searched.map(
    ({ label, path: candidate }) =>
      `plec:   - ${String(candidate)} (${label})`,
  );

  throw new Error(
    [
      Log.NoExecutableFound,
      Log.Searched,
      ...searchedCandidates,
      Log.BuildRequired,
      Log.InstallRequired,
    ].join('\n'),
  );
}

function resolveOverride(
  override: string | undefined,
): string | undefined {
  if (!override) {
    return undefined;
  }

  const candidate = path.resolve(override);

  if (usable(candidate)) {
    return candidate;
  }

  return failure([
    {
      label: Log.EnvOverride,
      path: candidate,
    },
  ]);
}

const binaryName =
  process.platform === Platform.Windows
    ? PlecExecutable.Windows
    : PlecExecutable.Default;

const packagedBinary = path.join(
  import.meta.dirname,
  BinaryPath.Packaged,
  binaryName,
);

const cargoBinary = path.join(
  os.homedir(),
  BinaryPath.Cargo,
  binaryName,
);

function resolveBinary(): ResolvedBinary {
  const override = resolveOverride(process.env.PLEC_BIN);

  if (override) {
    return { binary: override };
  }

  if (usable(packagedBinary)) {
    return { binary: packagedBinary };
  }

  if (usable(cargoBinary)) {
    return { binary: cargoBinary };
  }

  return failure([
    {
      label: Log.PackagedCLI,
      path: packagedBinary,
    },
    {
      label: Log.CompiledCLI,
      path: cargoBinary,
    },
  ]);
}

const { binary } = resolveBinary();

const child = spawn(binary, process.argv.slice(2), {
  stdio: 'inherit',
});

child.on('error', (error: Error) => {
  console.error(`plec: failed to run ${binary}: ${error.message}`);
  process.exit(1);
});

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }

  process.exit(code ?? 0);
});
