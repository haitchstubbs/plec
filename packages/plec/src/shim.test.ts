import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * The bin shim (bin/plec.js) is the package-owned `plec` command. These
 * checks exec the shim directly and skip when the artifact (whose packaged
 * release binary the shim resolves) has not been built yet.
 */
const packageDir = path.resolve(import.meta.dirname, '..');
const shimPath = path.join(packageDir, 'bin', 'plec.js');
const distBinary = path.join(
  packageDir,
  'dist',
  'bin',
  process.platform === 'win32' ? 'plec.exe' : 'plec',
);
const built = existsSync(distBinary);

function runShim(args: string[], env: NodeJS.ProcessEnv = {}) {
  return spawnSync(process.execPath, [shimPath, ...args], {
    encoding: 'utf8',
    env: { ...process.env, ...env },
  });
}

describe.skipIf(!built)('plec bin shim', () => {
  it('resolves the packaged release binary by default', () => {
    const result = runShim(['--help']);
    expect(result.status).toBe(0);
    // Release variant: app commands only, no dev `workspace` group.
    expect(result.stdout).toContain('Usage: plec');
    expect(result.stdout).not.toContain('workspace');
  });

  it('propagates nonzero exit codes from the binary', () => {
    const result = runShim(['build', '/nonexistent/app.tsx']);
    expect(result.status).not.toBe(0);
  });

  it('honors a PLEC_BIN override', () => {
    const cargoBinary = path.join(
      os.homedir(),
      '.cargo',
      'bin',
      process.platform === 'win32' ? 'plec.exe' : 'plec',
    );
    if (!existsSync(cargoBinary)) return;
    const result = runShim(['--help'], { PLEC_BIN: cargoBinary });
    expect(result.status).toBe(0);
    // The cargo-installed dev variant carries the workspace group.
    expect(result.stdout).toContain('workspace');
  });

  it('fails naming the override when PLEC_BIN is unusable', () => {
    const missing = path.join(
      packageDir,
      'dist',
      'bin',
      'does-not-exist',
    );
    const result = runShim(['--help'], { PLEC_BIN: missing });
    expect(result.status).toBe(1);
    expect(result.stderr).toContain('PLEC_BIN');
    expect(result.stderr).toContain(missing);
  });
});
