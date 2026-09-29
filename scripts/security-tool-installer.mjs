import { createHash } from 'node:crypto';
import {
  chmodSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import path from 'node:path';

export function securityToolPlatformKey(platform, arch) {
  return `${platform}-${arch}`;
}

export function securityToolReleaseAssetUrl(config, artifact) {
  const repository = new URL(config.repository);
  if (
    repository.protocol !== 'https:' ||
    repository.hostname !== 'github.com' ||
    repository.pathname.split('/').filter(Boolean).length !== 2
  ) {
    throw new Error(
      `Invalid official GitHub repository: ${config.repository}`,
    );
  }
  if (
    !artifact['release-tag'] ||
    !artifact.asset ||
    path.basename(artifact.asset) !== artifact.asset
  ) {
    throw new Error('Invalid pinned security-tool release metadata.');
  }
  return `${config.repository}/releases/download/${encodeURIComponent(
    artifact['release-tag'],
  ).replaceAll('%2F', '/')}/${artifact.asset}`;
}

function archiveEntries(archive, run) {
  const result = run('tar', ['--list', '--gzip', '--file', archive], {
    encoding: 'utf8',
    stdio: 'pipe',
  });
  if (result.error || result.status !== 0) {
    throw new Error(
      `Unable to inspect security-tool archive ${archive}.`,
    );
  }
  const entries = (result.stdout ?? '').split(/\r?\n/).filter(Boolean);
  for (const entry of entries) {
    const normalized = entry.replaceAll('\\', '/');
    if (
      normalized.startsWith('/') ||
      normalized.split('/').some((segment) => segment === '..')
    ) {
      throw new Error(`Unsafe path in security-tool archive: ${entry}`);
    }
  }
  return entries;
}

function findRegularFile(root, filename) {
  if (!existsSync(root)) return undefined;
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const candidate = path.join(root, entry.name);
    if (entry.isDirectory()) {
      const found = findRegularFile(candidate, filename);
      if (found) return found;
    } else if (
      entry.name === filename &&
      lstatSync(candidate).isFile()
    ) {
      return candidate;
    }
  }
  return undefined;
}

function installPrebuilt({
  name,
  config,
  artifact,
  root,
  executable,
  repoRoot,
  run,
  versionOf,
}) {
  if (!/^[a-f0-9]{64}$/.test(artifact.sha256 ?? '')) {
    throw new Error(
      `Invalid pinned SHA-256 for ${name} ${config.version}.`,
    );
  }
  const archive = path.join(
    repoRoot,
    '.cache',
    `${name}-${config.version}.${artifact.asset.endsWith('.tgz') ? 'tgz' : 'tar.gz'}`,
  );
  const stage = `${root}.extracting`;
  mkdirSync(path.dirname(archive), { recursive: true });
  mkdirSync(path.dirname(root), { recursive: true });

  console.log(
    `Downloading ${name} official release ${config.version}...`,
  );
  const download = run('curl', [
    '--fail',
    '--location',
    '--silent',
    '--show-error',
    '--output',
    archive,
    securityToolReleaseAssetUrl(config, artifact),
  ]);
  if (download.error || download.status !== 0) {
    rmSync(archive, { force: true });
    throw new Error(
      `Failed to download ${name} ${config.version} (${artifact.asset}).`,
    );
  }
  const actual = createHash('sha256')
    .update(readFileSync(archive))
    .digest('hex');
  if (actual !== artifact.sha256) {
    rmSync(archive, { force: true });
    throw new Error(
      `${name} ${config.version} checksum mismatch for ${artifact.asset}: expected ${artifact.sha256}, found ${actual}.`,
    );
  }
  console.log(`${name} ${config.version} checksum verified.`);

  rmSync(stage, { recursive: true, force: true });
  mkdirSync(stage, { recursive: true });
  try {
    archiveEntries(archive, run);
    const extract = run('tar', [
      '--extract',
      '--gzip',
      '--file',
      archive,
      '--no-same-owner',
      '--no-same-permissions',
      '--directory',
      stage,
    ]);
    if (extract.error || extract.status !== 0) {
      throw new Error(
        `Failed to extract ${name} ${config.version} (${artifact.asset}).`,
      );
    }
    const extracted = findRegularFile(stage, executable);
    if (!extracted) {
      throw new Error(
        `${artifact.asset} did not contain expected executable ${executable}.`,
      );
    }
    chmodSync(extracted, 0o755);
    const bytes = readFileSync(extracted);
    rmSync(stage, { recursive: true, force: true });
    const stagedBinary = path.join(
      stage,
      'bin',
      path.basename(executable),
    );
    mkdirSync(path.dirname(stagedBinary), { recursive: true });
    writeFileSync(stagedBinary, bytes, { mode: 0o755 });
    const actualVersion = versionOf(stagedBinary);
    if (actualVersion !== config.version) {
      throw new Error(
        `${name} version mismatch after extracting ${artifact.asset}: expected ${config.version}, found ${actualVersion ?? 'missing'}.`,
      );
    }
    rmSync(root, { recursive: true, force: true });
    renameSync(stage, root);
  } finally {
    rmSync(archive, { force: true });
    rmSync(stage, { recursive: true, force: true });
  }
}

export function ensureSecurityTool({
  name,
  config,
  artifacts,
  platform,
  arch,
  ci,
  root,
  executable,
  repoRoot,
  run,
  versionOf,
}) {
  if (
    existsSync(executable) &&
    versionOf(executable) === config.version
  ) {
    console.log(`${name} found: ${config.version}`);
    return;
  }
  const artifact =
    artifacts?.[securityToolPlatformKey(platform, arch)]?.[name];
  if (artifact) {
    installPrebuilt({
      name,
      config,
      artifact,
      root,
      executable: path.basename(executable),
      repoRoot,
      run,
      versionOf,
    });
  } else {
    if (ci) {
      throw new Error(
        `No pinned prebuilt ${name} ${config.version} is configured for ${platform}/${arch}; refusing a source build in CI.`,
      );
    }
    console.log(
      `Installing pinned ${name} ${config.version} from source...`,
    );
    const args = [
      'install',
      '--git',
      config.repository,
      '--rev',
      config.commit,
      '--locked',
      '--root',
      root,
      '--force',
    ];
    if (config.features?.length)
      args.push('--features', config.features.join(','));
    args.push(config.package ?? name);
    const install = run('cargo', args, { cwd: repoRoot });
    if (install.error || install.status !== 0) {
      throw new Error(
        `Failed to install pinned ${name} from ${config.commit}.`,
      );
    }
  }

  const actual = versionOf(executable);
  if (actual !== config.version) {
    throw new Error(
      `${name} version mismatch after installation: expected ${config.version}, found ${actual ?? 'missing'} at ${executable}.`,
    );
  }
  console.log(`${name} ${config.version} ready.`);
}
