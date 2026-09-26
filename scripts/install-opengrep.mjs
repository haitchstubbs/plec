import { chmodSync, mkdirSync, readFileSync, rmSync } from 'node:fs';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import cliToolsConfig from '../cli-tools.json' with { type: 'json' };

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const version = cliToolsConfig['build-tools'].opengrep;
const platform = `${process.platform}-${process.arch}`;
const asset =
  cliToolsConfig['prebuilt-build-tools']?.[platform]?.opengrep;

function run(binary, args) {
  return spawnSync(binary, args, { encoding: 'utf8', shell: false });
}

export function ensureOpengrep() {
  if (!asset) {
    throw new Error(
      `No pinned OpenGrep build is configured for ${platform}.`,
    );
  }

  const root = path.join(repoRoot, '.tools', `opengrep-${version}`);
  const binary = path.join(root, 'opengrep');
  const installed = run(binary, ['--version']);
  if (
    !installed.error &&
    installed.status === 0 &&
    installed.stdout.includes(version)
  ) {
    console.log(`opengrep found: ${installed.stdout.trim()}`);
    return binary;
  }

  mkdirSync(root, { recursive: true });
  const download = run('curl', [
    '--fail',
    '--location',
    '--silent',
    '--show-error',
    '--output',
    binary,
    `https://github.com/opengrep/opengrep/releases/download/${asset.target}/${asset.asset}`,
  ]);
  if (download.error || download.status !== 0) {
    rmSync(binary, { force: true });
    throw new Error(`Failed to download pinned OpenGrep ${version}.`);
  }

  const actual = createHash('sha256')
    .update(readFileSync(binary))
    .digest('hex');
  if (actual !== asset.sha256) {
    rmSync(binary, { force: true });
    throw new Error(
      `Pinned OpenGrep checksum mismatch: expected ${asset.sha256}, found ${actual}.`,
    );
  }

  chmodSync(binary, 0o755);
  const check = run(binary, ['--version']);
  if (
    check.error ||
    check.status !== 0 ||
    !check.stdout.includes(version)
  ) {
    rmSync(binary, { force: true });
    throw new Error(
      `Downloaded OpenGrep did not report version ${version}.`,
    );
  }

  console.log(
    `opengrep ${version} (${asset.commit}) installed at ${binary}`,
  );
  return binary;
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    ensureOpengrep();
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
