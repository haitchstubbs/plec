import { execFileSync } from 'node:child_process';
import { mkdir, rm } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { findInPath, isWindows } from './utils.js';

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
 * @property {boolean} [optimize=true] - Whether to run wasm-opt/wasm-tools strip
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
    const relativeOutDir = relativePath(absoluteCratePath, absoluteOutDir);

    console.log(`Building WASM: ${absoluteCratePath} -> ${absoluteOutDir} (relative: ${relativeOutDir})`);

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

    // Add feature flags if specified
    if (selectedFeatures) {
      const noDefaultFlags =
        profile === 'core' || profile === 'router' || profile === 'fetch'
          ? ['--no-default-features']
          : [];
      args.push(
        '--',
        ...noDefaultFlags,
        ...(selectedFeatures.length ? ['--features', selectedFeatures.join(',')] : []),
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
      },
    });

    // Optimize if requested
    if (optimize) {
      await optimizeWasm(outDir, outName);
    }

    return { wasmPath: path.join(outDir, `${outName}_bg.wasm`) };
  } finally {
    // Clean up temp directory
    await rm(temporaryDirectory, { recursive: true, force: true });
  }
}

/**
 * Optimize WASM file using wasm-tools strip.
 *
 * @param {string} outDir - Output directory containing WASM file
 * @param {string} outName - Name of the WASM file (without extension)
 */
async function optimizeWasm(outDir, outName) {
  const wasmTools = await findWasmTools();

  if (!wasmTools) {
    console.warn('wasm-tools not found in PATH, skipping WASM optimization');
    return;
  }

  const wasmPath = path.join(outDir, `${outName}_bg.wasm`);
  const strippedWasmPath = path.join(outDir, `${outName}_bg.stripped.wasm`);

  console.log(`Optimizing WASM with wasm-tools...`);

  execFileSync(
    wasmTools,
    ['strip', '--all', wasmPath, '-o', strippedWasmPath],
    {
      cwd: repoRoot,
      stdio: 'inherit',
    },
  );

  await rm(wasmPath, { force: true });

  // Rename stripped version back to original name
  await renameFile(strippedWasmPath, wasmPath);

  const stats = await import('node:fs/promises').then(fs => fs.stat(wasmPath));
  console.log(`Optimized WASM size: ${stats.size} bytes`);
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
 * Find wasm-tools executable in PATH.
 *
 * @returns {Promise<string|null>} Path to wasm-tools or null if not found
 */
async function findWasmTools() {
  const exeName = isWindows() ? 'wasm-tools.exe' : 'wasm-tools';
  return await findInPath(exeName);
}

// CLI interface for direct execution
if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
) {
  const args = process.argv.slice(2);

  if (args.length === 0) {
    console.error('Usage: build-wasm.mjs <crate-path> [out-dir] [options]');
    process.exit(1);
  }

  const cratePath = args[0];
  const outDir = args[1] || path.join(path.dirname(cratePath), 'dist', 'runtime');

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
