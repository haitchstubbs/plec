import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import {
  ensureSecurityTool,
  securityToolReleaseAssetUrl,
} from './security-tool-installer.mjs';

const config = {
  version: '0.19.0',
  repository: 'https://github.com/EmbarkStudios/cargo-deny',
  commit: '09faadcea2d0d1742492e6872b743d1e4d151a27',
};
const artifact = {
  'release-tag': '0.19.0',
  asset: 'cargo-deny-0.19.0-x86_64-unknown-linux-musl.tar.gz',
};

function fixture() {
  const repoRoot = mkdtempSync(
    path.join(os.tmpdir(), 'plec-security-tools-'),
  );
  const root = path.join(
    repoRoot,
    '.tools',
    'cargo',
    'cargo-deny',
    config.commit,
  );
  const executable = path.join(root, 'bin', 'cargo-deny');
  const archiveSource = path.join(repoRoot, 'fixture.tar.gz');
  const wrappedRoot = path.join(
    repoRoot,
    'wrapped',
    'cargo-deny-release',
  );
  mkdirSync(wrappedRoot, { recursive: true });
  writeFileSync(path.join(wrappedRoot, 'cargo-deny'), config.version);
  const packed = spawnSync('tar', [
    '--create',
    '--gzip',
    '--file',
    archiveSource,
    '--directory',
    path.join(repoRoot, 'wrapped'),
    'cargo-deny-release',
  ]);
  assert.equal(packed.status, 0);
  const bytes = readFileSync(archiveSource);
  const pinnedArtifact = {
    ...artifact,
    sha256: createHash('sha256').update(bytes).digest('hex'),
  };
  const calls = [];
  const run = (command, args, options = {}) => {
    calls.push({ command, args });
    if (command === 'curl') {
      const output = args[args.indexOf('--output') + 1];
      copyFileSync(archiveSource, output);
      return { status: 0 };
    }
    return spawnSync(command, args, {
      encoding: 'utf8',
      ...options,
    });
  };
  const versionOf = (binary) => {
    try {
      return readFileSync(binary, 'utf8');
    } catch {
      return undefined;
    }
  };
  return {
    repoRoot,
    root,
    executable,
    archiveSource,
    pinnedArtifact,
    calls,
    run,
    versionOf,
    cleanup: () => rmSync(repoRoot, { recursive: true, force: true }),
  };
}

function ensure(options, state) {
  ensureSecurityTool({
    name: 'cargo-deny',
    config,
    artifacts: { 'linux-x64': { 'cargo-deny': state.pinnedArtifact } },
    platform: 'linux',
    arch: 'x64',
    ci: true,
    root: state.root,
    executable: state.executable,
    repoRoot: state.repoRoot,
    run: state.run,
    versionOf: state.versionOf,
    ...options,
  });
}

test('release asset URL uses the exact official pinned tag and asset', () => {
  assert.equal(
    securityToolReleaseAssetUrl(config, artifact),
    'https://github.com/EmbarkStudios/cargo-deny/releases/download/0.19.0/cargo-deny-0.19.0-x86_64-unknown-linux-musl.tar.gz',
  );
  assert.equal(
    securityToolReleaseAssetUrl(
      { repository: 'https://github.com/RustSec/rustsec' },
      {
        ...artifact,
        'release-tag': 'cargo-audit/v0.22.1',
        asset: 'cargo-audit.tgz',
      },
    ),
    'https://github.com/RustSec/rustsec/releases/download/cargo-audit/v0.22.1/cargo-audit.tgz',
  );
});

test('correct cached version returns without download or install', (context) => {
  const state = fixture();
  context.after(state.cleanup);
  mkdirSync(path.dirname(state.executable), { recursive: true });
  writeFileSync(state.executable, config.version);
  ensure({}, state);
  assert.deepEqual(state.calls, []);
});

test('prebuilt replaces a wrong cached version and verifies extracted version', (context) => {
  const state = fixture();
  context.after(state.cleanup);
  mkdirSync(path.dirname(state.executable), { recursive: true });
  writeFileSync(state.executable, '0.18.0');
  ensure({}, state);
  assert.equal(state.versionOf(state.executable), config.version);
  assert.equal(
    state.calls.filter(({ command }) => command === 'curl').length,
    1,
  );
});

test('checksum mismatch is rejected before extraction', (context) => {
  const state = fixture();
  context.after(state.cleanup);
  state.pinnedArtifact.sha256 = '0'.repeat(64);
  assert.throws(() => ensure({}, state), /checksum mismatch/);
  assert.equal(
    state.calls.some(
      ({ command, args }) =>
        command === 'tar' && args.includes('--extract'),
    ),
    false,
  );
  assert.equal(
    state.calls.some(({ command }) => command === 'cargo'),
    false,
  );
});

test('missing prebuilt fails in CI and uses the pinned source fallback locally', (context) => {
  const state = fixture();
  context.after(state.cleanup);
  assert.throws(
    () => ensure({ artifacts: {}, ci: true }, state),
    /refusing a source build in CI/,
  );
  ensure(
    {
      artifacts: {},
      ci: false,
      run: (command, args) => {
        state.calls.push({ command, args });
        mkdirSync(path.dirname(state.executable), { recursive: true });
        writeFileSync(state.executable, config.version);
        return { status: 0 };
      },
    },
    state,
  );
  const cargoInstall = state.calls.find(
    ({ command }) => command === 'cargo',
  );
  assert.ok(cargoInstall.args.includes(config.commit));
  assert.ok(cargoInstall.args.includes('--locked'));
  assert.equal(state.versionOf(state.executable), config.version);
});
