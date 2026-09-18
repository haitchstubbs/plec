import { execFileSync } from 'node:child_process';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { brotliCompress, constants as zlibConstants } from 'node:zlib';
import { promisify } from 'node:util';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { findInPath, isWindows } from './utils.js';
import { wasmBindgenBinPath, wasmPackCachePath } from './toolchain.mjs';

const brotliCompressAsync = promisify(brotliCompress);
const require = createRequire(import.meta.url);
/**
 * Get relative path from one directory to another.
 *
 * @param {string} from - Source directory
 * @param {string} to - Target directory/file
 * @returns {string} Relative path
 */
function relativePath(from, to) {
  const fromPath = path.resolve(from);
  const toPath = path.resolve(to);
  const relative = path.relative(fromPath, toPath);
  return relative;
}

/**
 * Shared WASM build utility for the plec-runtime package.
 *
 * This utility:
 * - Invokes wasm-pack with cross-platform path handling
 * - Manages temporary directory cleanup
 * - Supports feature profiles and custom features
 * - Outputs to a specified directory
 *
 * @typedef {Object} BuildWasmOptions
 * @property {string} cratePath - Path to the Rust crate directory
 * @property {string} outDir - Output directory for WASM files
 * @property {string} [outName='runtime'] - Name for the output WASM file
 * @property {string[]} [features] - Optional features to build
 * @property {'full'|'core'|'router'|'fetch'} [profile='full'] - Feature profile
 * @property {boolean} [optimize=true] - Whether to run Binaryen and strip metadata
 */

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);

const temporaryDirectory = path.join(repoRoot, '.tmp', 'wasm-pack');

/**
 * Build WASM using wasm-pack.
 *
 * @param {BuildWasmOptions} options
 */
export async function buildWasm({
  cratePath,
  outDir,
  outName = 'runtime',
  features,
  profile = 'full',
  optimize = true,
}) {
  // Resolve paths to absolute paths
  const absoluteCratePath = path.resolve(cratePath);
  const absoluteOutDir = path.resolve(outDir);

  // Clean up any existing temp directory
  await rm(temporaryDirectory, { recursive: true, force: true });
  await mkdir(temporaryDirectory, { recursive: true });

  try {
    // Determine features based on profile
    const profileFeatures = {
      full: undefined,
      core: [],
      router: ['router'],
      fetch: ['fetch'],
    }[profile];

    if (profileFeatures === undefined && profile !== 'full') {
      throw new Error(`Unknown profile: ${profile}`);
    }

    const selectedFeatures = features ?? profileFeatures;
    // wasm-pack interprets --out-dir relative to the crate directory
    const relativeOutDir = relativePath(
      absoluteCratePath,
      absoluteOutDir,
    );

    console.log(
      `Building WASM: ${absoluteCratePath} -> ${absoluteOutDir} (relative: ${relativeOutDir})`,
    );

    const args = [
      'build',
      absoluteCratePath,
      '--target',
      'web',
      '--out-dir',
      absoluteOutDir,
      '--out-name',
      outName,
    ];

    // Cargo must use the workspace lockfile when wasm-pack builds the crate.
    args.push('--', '--locked');

    // Add feature flags if specified
    if (selectedFeatures) {
      const noDefaultFlags =
        profile === 'core' ||
        profile === 'router' ||
        profile === 'fetch'
          ? ['--no-default-features']
          : [];
      args.push(
        ...noDefaultFlags,
        ...(selectedFeatures.length
          ? ['--features', selectedFeatures.join(',')]
          : []),
      );
    }

    const wasmOpt = await findWasmOpt();
    if (!wasmOpt) {
      throw new Error(
        'wasm-opt is required by the pinned binaryen dependency; run yarn install',
      );
    }

    // Run wasm-pack
    execFileSync('wasm-pack', args, {
      cwd: repoRoot,
      stdio: 'inherit',
      env: {
        ...process.env,
        TMP: temporaryDirectory,
        TEMP: temporaryDirectory,
        WASM_PACK_CACHE: wasmPackCachePath,
        PATH: [
          wasmBindgenBinPath(),
          path.dirname(wasmOpt),
          process.env.PATH,
        ]
          .filter(Boolean)
          .join(path.delimiter),
      },
    });

    // Optimize if requested
    if (optimize) {
      await optimizeWasm(outDir, outName);
    }

    // Stamp the implemented protocol versions into the binary and record
    // build identity, so staleness is detectable downstream
    // (`plec workspace artifact provenance` / `plec workspace artifact stale`).
    await stampProtocolSection(
      path.join(absoluteOutDir, `${outName}_bg.wasm`),
    );
    await writeBrotliSidecars(absoluteOutDir, outName);
    await writeProvenance(absoluteOutDir, {
      outName,
      profile,
      features: selectedFeatures,
      optimize,
    });

    return { wasmPath: path.join(outDir, `${outName}_bg.wasm`) };
  } finally {
    // Clean up temp directory
    await rm(temporaryDirectory, { recursive: true, force: true });
  }
}

/**
 * Optimize WASM with Binaryen, then remove metadata sections.
 *
 * @param {string} outDir - Output directory containing WASM file
 * @param {string} outName - Name of the WASM file (without extension)
 */
async function optimizeWasm(outDir, outName) {
  const wasmOpt = await findWasmOpt();
  const wasmTools = await findWasmTools();

  if (!wasmOpt) {
    throw new Error(
      'wasm-opt is required by the pinned binaryen dependency; run yarn install',
    );
  }
  if (!wasmTools) {
    throw new Error('wasm-tools is required by the pinned toolchain');
  }

  const wasmPath = path.join(outDir, `${outName}_bg.wasm`);
  const optimizedWasmPath = path.join(
    outDir,
    `${outName}_bg.optimized.wasm`,
  );
  const strippedWasmPath = path.join(
    outDir,
    `${outName}_bg.stripped.wasm`,
  );

  console.log(`Optimizing WASM with wasm-opt -Oz...`);
  execFileSync(
    wasmOpt,
    [
      '-Oz',
      '--strip-debug',
      '--strip-producers',
      wasmPath,
      '-o',
      optimizedWasmPath,
    ],
    { cwd: repoRoot, stdio: 'inherit' },
  );

  console.log(`Stripping WASM metadata with wasm-tools...`);
  execFileSync(
    wasmTools,
    ['strip', '--all', optimizedWasmPath, '-o', strippedWasmPath],
    {
      cwd: repoRoot,
      stdio: 'inherit',
    },
  );

  await rm(wasmPath, { force: true });
  await rm(optimizedWasmPath, { force: true });

  // Rename stripped version back to original name
  await renameFile(strippedWasmPath, wasmPath);

  const stats = await import('node:fs/promises').then((fs) =>
    fs.stat(wasmPath),
  );
  console.log(`Optimized WASM size: ${stats.size} bytes`);
}

/**
 * Find Binaryen's wasm-opt, including the executable shipped by the Yarn
 * dependency when Plug'n'Play is enabled.
 *
 * @returns {Promise<string|null>} Path to wasm-opt or null if not found
 */
async function findWasmOpt() {
  const exeName = isWindows() ? 'wasm-opt.exe' : 'wasm-opt';
  const inPath = await findInPath(exeName);
  if (inPath) return inPath;

  try {
    const binaryenEntry = require.resolve('binaryen');
    return path.join(path.dirname(binaryenEntry), 'bin', exeName);
  } catch {
    return null;
  }
}

/**
 * Rename a file (cross-platform replacement for fs.rename).
 *
 * @param {string} oldPath - Current file path
 * @param {string} newPath - New file path
 */
async function renameFile(oldPath, newPath) {
  const fs = await import('node:fs/promises');
  try {
    await fs.rename(oldPath, newPath);
  } catch (error) {
    // Fall back to copy + delete if rename fails (e.g., cross-device)
    if (error.code === 'EXDEV') {
      await fs.copyFile(oldPath, newPath);
      await fs.unlink(oldPath);
    } else {
      throw error;
    }
  }
}

/**
 * Emit maximum-quality Brotli sidecars (`<artifact>.br`) next to the runtime
 * artifacts. The app stager (crates/plec-cli/src/com/stage.rs) copies them
 * when present and the native `plec-server` serves them negotiated by
 * `Accept-Encoding`; every rebuild overwrites them so a sidecar can never
 * outlive the bytes it compresses.
 *
 * @param {string} outDir - Output directory containing the artifacts
 * @param {string} outName - Name of the artifacts (without extension)
 */
async function writeBrotliSidecars(outDir, outName) {
  const params = {
    [zlibConstants.BROTLI_PARAM_QUALITY]:
      zlibConstants.BROTLI_MAX_QUALITY,
  };
  for (const name of [`${outName}.js`, `${outName}_bg.wasm`]) {
    const filePath = path.join(outDir, name);
    const source = await readFile(filePath);
    const compressed = await brotliCompressAsync(source, { params });
    await writeFile(`${filePath}.br`, compressed);
    console.log(
      `Brotli sidecar: ${name} ${source.length} -> ${compressed.length} bytes`,
    );
  }
}

/**
 * Find wasm-tools executable in PATH.
 *
 * @returns {Promise<string|null>} Path to wasm-tools or null if not found
 */
async function findWasmTools() {
  const exeName = isWindows() ? 'wasm-tools.exe' : 'wasm-tools';
  return await findInPath(exeName);
}

// ---------------------------------------------------------------------------
// Protocol stamping and build provenance
//
// The runtime embeds the serialized protocol versions it implements in a
// `plec-protocol` WASM custom section (see crates/plec-runtime/src/lib.rs,
// which emits the section from the plec-ir constants at compile time). The
// Rust static can be dropped by toolchain stripping, so this step guarantees
// the section exists in the final artifact: verify first, append if missing.
// `provenance.json` records build identity so `plec workspace artifact provenance`
// can distinguish source, package dist, and staged app artifacts.
// ---------------------------------------------------------------------------

const PROTOCOL_SECTION_NAME = 'plec-protocol';

/**
 * Read `SSR_SNAPSHOT_VERSION` from the plec-ir source. The Rust constant is
 * the single source of truth; this parses the source text so the build
 * script never hard-codes a protocol version.
 */
function readSsrSnapshotVersion() {
  const source = readFileSync(
    path.join(repoRoot, 'crates', 'plec-ir', 'src', 'lib.rs'),
    'utf8',
  );
  const match = /pub const SSR_SNAPSHOT_VERSION: u32 = (\d+);/.exec(
    source,
  );
  if (!match) {
    throw new Error(
      'SSR_SNAPSHOT_VERSION not found in crates/plec-ir/src/lib.rs',
    );
  }
  return Number(match[1]);
}

function encodeLeb(value) {
  const bytes = [];
  let remaining = value;
  do {
    let byte = remaining & 0x7f;
    remaining >>>= 7;
    if (remaining !== 0) byte |= 0x80;
    bytes.push(byte);
  } while (remaining !== 0);
  return bytes;
}

function decodeLeb(bytes, offset) {
  let result = 0;
  let shift = 0;
  let position = offset;
  for (;;) {
    const byte = bytes[position++];
    result += (byte & 0x7f) * 2 ** shift;
    if ((byte & 0x80) === 0) break;
    shift += 7;
  }
  return [result, position];
}

function customSectionBytes(name, payload) {
  const nameBytes = [...Buffer.from(name, 'utf8')];
  const nameLengthBytes = encodeLeb(nameBytes.length);
  const sectionPayloadLength =
    nameLengthBytes.length + nameBytes.length + payload.length;
  return [
    0,
    ...encodeLeb(sectionPayloadLength),
    ...nameLengthBytes,
    ...nameBytes,
    ...payload,
  ];
}

/**
 * Locate a custom section's payload in a WASM binary.
 *
 * @returns {Buffer|null} payload bytes, or null when absent
 */
function findCustomSection(wasm, name) {
  if (wasm.length < 8 || wasm[0] !== 0x00 || wasm[1] !== 0x61)
    return null;
  let position = 8;
  while (position < wasm.length) {
    const id = wasm[position++];
    const [size, payloadStart] = decodeLeb(wasm, position);
    position = payloadStart;
    if (id === 0) {
      const [nameLength, nameStart] = decodeLeb(wasm, position);
      const nameEnd = nameStart + nameLength;
      if (
        wasm.subarray(nameStart, nameEnd).toString('utf8') === name &&
        nameEnd <= payloadStart + size
      ) {
        return wasm.subarray(nameEnd, payloadStart + size);
      }
    }
    position += size;
  }
  return null;
}

async function stampProtocolSection(wasmPath) {
  const version = readSsrSnapshotVersion();
  const expected = Buffer.from(
    JSON.stringify({ ssrSnapshot: version }),
  );

  const wasm = await readFile(wasmPath);
  const existing = findCustomSection(wasm, PROTOCOL_SECTION_NAME);

  if (existing) {
    if (!existing.equals(expected)) {
      throw new Error(
        `${PROTOCOL_SECTION_NAME} section in ${wasmPath} reports ` +
          `${existing.toString('utf8')} but plec-ir says ${expected.toString('utf8')} — ` +
          'the crate needs a rebuild before stamping',
      );
    }
    console.log(
      `Protocol section present: ${PROTOCOL_SECTION_NAME} ${expected.toString('utf8')}`,
    );
    return;
  }

  // A custom section may appear anywhere between other sections, including
  // at the end of the file.
  const stamped = Buffer.concat([
    wasm,
    Buffer.from(customSectionBytes(PROTOCOL_SECTION_NAME, expected)),
  ]);
  await writeFile(wasmPath, stamped);
  console.log(
    `Injected protocol section: ${PROTOCOL_SECTION_NAME} ${expected.toString('utf8')}`,
  );
}

async function writeProvenance(
  outDir,
  { outName, profile, features, optimize },
) {
  const wasmPath = path.join(outDir, `${outName}_bg.wasm`);
  const jsPath = path.join(outDir, `${outName}.js`);

  const sha256 = (filePath) =>
    createHash('sha256').update(readFileSync(filePath)).digest('hex');

  let gitFingerprint = '';
  let gitDirty = false;
  try {
    gitFingerprint = execFileSync('git', ['rev-parse', 'HEAD'], {
      cwd: repoRoot,
      encoding: 'utf8',
    }).trim();
    gitDirty =
      execFileSync('git', ['status', '--porcelain'], {
        cwd: repoRoot,
        encoding: 'utf8',
      }).trim().length > 0;
  } catch {
    // Not a git checkout (or git unavailable): provenance still records
    // hashes and timestamps.
  }

  const provenance = {
    builtAt: new Date().toISOString(),
    builder: 'scripts/build-wasm.mjs',
    crate: 'crates/plec-runtime',
    profile,
    features: features ?? null,
    optimize,
    gitFingerprint: gitFingerprint.slice(0, 12),
    gitDirty,
    wasmSha256: sha256(wasmPath),
    jsSha256: sha256(jsPath),
    protocol: {
      ssrSnapshot: readSsrSnapshotVersion(),
    },
  };

  await writeFile(
    path.join(outDir, 'provenance.json'),
    JSON.stringify(provenance, null, 2) + '\n',
  );
  console.log(
    `Provenance written: ${path.join(outDir, 'provenance.json')}`,
  );
}

// CLI interface for direct execution
if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
) {
  const args = process.argv.slice(2);

  if (args.length === 0) {
    console.error(
      'Usage: build-wasm.mjs <crate-path> [out-dir] [options]',
    );
    process.exit(1);
  }

  const cratePath = args[0];
  const outDir =
    args[1] || path.join(path.dirname(cratePath), 'dist', 'runtime');

  // Parse options
  const options = { cratePath, outDir, optimize: true };
  for (let i = 2; i < args.length; i++) {
    const arg = args[i];
    if (arg === '--profile') {
      options.profile = args[++i];
    } else if (arg === '--features') {
      options.features = args[++i].split(',');
    } else if (arg === '--no-optimize') {
      options.optimize = false;
    } else if (arg === '--out-name') {
      options.outName = args[++i];
    }
  }

  buildWasm(options).catch((error) => {
    console.error('Build failed:', error);
    process.exit(1);
  });
}
