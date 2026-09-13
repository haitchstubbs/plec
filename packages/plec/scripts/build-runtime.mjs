#!/usr/bin/env node
import { createHash, randomUUID } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import {
  cp,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rename,
  rm,
  rmdir,
} from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { brotliDecompressSync } from 'node:zlib';

const packageDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const repoRoot = path.resolve(packageDir, '..', '..');
const releaseRoot = path.join(repoRoot, '.tmp', 'release');
const destinationDir = path.join(packageDir, 'dist', 'runtime');
const buildArgs = process.argv.slice(2);

const REQUIRED_ASSETS = [
  'runtime.js',
  'runtime_bg.wasm',
  'runtime.js.br',
  'runtime_bg.wasm.br',
  'provenance.json',
];

await mkdir(releaseRoot, { recursive: true });
const temporaryDir = await mkdtemp(path.join(releaseRoot, 'runtime-'));
const publishDir = path.join(
  packageDir,
  'dist',
  `.runtime-publish-${randomUUID()}`,
);
const previousDir = path.join(
  packageDir,
  'dist',
  `.runtime-previous-${randomUUID()}`,
);

try {
  execFileSync(process.execPath, ['scripts/verify-toolchain.mjs'], {
    cwd: repoRoot,
    stdio: 'inherit',
  });
  execFileSync(
    process.execPath,
    [
      'scripts/build-wasm.mjs',
      'crates/plec-runtime',
      temporaryDir,
      ...buildArgs,
    ],
    { cwd: repoRoot, stdio: 'inherit' },
  );

  await verifyRuntime(temporaryDir);
  await mkdir(path.dirname(destinationDir), { recursive: true });
  await cp(temporaryDir, publishDir, { recursive: true });
  await verifyRuntime(publishDir);

  // Publish only a complete, validated runtime tree. Keep the previous tree
  // until the replacement is in place so a failed swap remains recoverable.
  await publishRuntime();
  console.log(`Published runtime assets: ${destinationDir}`);
} finally {
  await rm(temporaryDir, { recursive: true, force: true });
  await rm(publishDir, { recursive: true, force: true });
  await removeReleaseRootIfEmpty();
}

async function verifyRuntime(directory) {
  const assets = new Map();
  for (const asset of REQUIRED_ASSETS) {
    const assetPath = path.join(directory, asset);
    try {
      assets.set(asset, await readFile(assetPath));
    } catch (error) {
      throw new Error(
        `missing runtime asset ${assetPath}: ${error.message}`,
      );
    }
  }

  const provenance = JSON.parse(
    assets.get('provenance.json').toString(),
  );
  for (const [asset, field] of [
    ['runtime.js', 'jsSha256'],
    ['runtime_bg.wasm', 'wasmSha256'],
  ]) {
    const expected = provenance[field];
    const actual = sha256(assets.get(asset));
    if (
      typeof expected !== 'string' ||
      expected.toLowerCase() !== actual
    ) {
      throw new Error(`runtime provenance hash mismatch for ${asset}`);
    }
  }

  for (const asset of ['runtime.js', 'runtime_bg.wasm']) {
    const sidecar = assets.get(`${asset}.br`);
    let decompressed;
    try {
      decompressed = brotliDecompressSync(sidecar);
    } catch (error) {
      throw new Error(
        `invalid Brotli sidecar for ${asset}: ${error.message}`,
      );
    }
    if (!decompressed.equals(assets.get(asset)))
      throw new Error(`Brotli sidecar does not match ${asset}`);
  }

  const protocol = protocolSection(assets.get('runtime_bg.wasm'));
  if (!protocol || !Number.isInteger(protocol.ssrSnapshot))
    throw new Error(
      'runtime WASM is missing a valid plec-protocol section',
    );
}

async function publishRuntime() {
  let movedPrevious = false;
  try {
    await rename(destinationDir, previousDir);
    movedPrevious = true;
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }

  try {
    await rename(publishDir, destinationDir);
  } catch (error) {
    if (movedPrevious) await rename(previousDir, destinationDir);
    throw error;
  }

  if (movedPrevious)
    await rm(previousDir, { recursive: true, force: true });
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function protocolSection(wasm) {
  if (wasm.subarray(0, 4).toString('ascii') !== '\0asm')
    return undefined;

  let offset = 8;
  while (offset < wasm.length) {
    const id = wasm[offset++];
    const size = readVarUint(wasm, offset);
    offset = size.offset;
    const end = offset + size.value;
    if (end > wasm.length) return undefined;

    if (id === 0) {
      const name = readVarUint(wasm, offset);
      const nameEnd = name.offset + name.value;
      if (nameEnd > end) return undefined;
      if (
        wasm.subarray(name.offset, nameEnd).toString('utf8') ===
        'plec-protocol'
      ) {
        try {
          return JSON.parse(
            wasm.subarray(nameEnd, end).toString('utf8'),
          );
        } catch {
          return undefined;
        }
      }
    }
    offset = end;
  }
  return undefined;
}

function readVarUint(bytes, offset) {
  let value = 0;
  let shift = 0;
  while (offset < bytes.length && shift <= 28) {
    const byte = bytes[offset++];
    value |= (byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) return { value, offset };
    shift += 7;
  }
  throw new Error('invalid WASM LEB128 value');
}

async function removeReleaseRootIfEmpty() {
  try {
    if ((await readdir(releaseRoot)).length === 0)
      await rmdir(releaseRoot);
  } catch {
    // A concurrent runtime build owns the directory or cleanup already won.
  }
}
