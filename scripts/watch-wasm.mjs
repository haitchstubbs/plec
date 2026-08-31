import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { watch } from 'node:fs/promises';

const packageRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);

const cratePath = path.join(packageRoot, 'crates', 'plec-runtime');
const outDir = path.join(packageRoot, 'dist', 'runtime');

// Feature/profile from environment
const profile = process.env.PLEC_RUNTIME_PROFILE ?? 'full';
const features = process.env.PLEC_RUNTIME_FEATURES?.split(',')
  .map((f) => f.trim())
  .filter(Boolean);

console.log(
  `WASM Watch Mode: Building WASM for profile "${profile}"...`,
);

// Initial build
await buildWasm();

console.log(
  `WASM Watch Mode: Watching ${cratePath}/src for changes...`,
);

// Watch for changes
const watcher = watch(path.join(cratePath, 'src'), { recursive: true });
let rebuildTimeout = null;

for await (const event of watcher) {
  if (event.filename && event.filename.endsWith('.rs')) {
    console.log(
      `\nWASM Watch Mode: ${event.filename} changed, rebuilding...`,
    );

    // Debounce rebuilds
    if (rebuildTimeout) {
      clearTimeout(rebuildTimeout);
    }

    rebuildTimeout = setTimeout(async () => {
      try {
        await buildWasm();
        console.log('WASM Watch Mode: Build complete.');
      } catch (error) {
        console.error('WASM Watch Mode: Build failed:', error.message);
      }
    }, 500);
  }
}

async function buildWasm() {
  const buildScript = path.resolve(
    packageRoot,
    '..',
    '..',
    'scripts',
    'build-wasm.mjs',
  );

  const args = [cratePath, outDir, '--profile', profile];
  if (features && features.length) {
    args.push('--features', features.join(','));
  }

  const proc = spawn('node', [buildScript, ...args], {
    stdio: 'inherit',
    cwd: packageRoot,
  });

  await new Promise((resolve, reject) => {
    proc.on('close', (code) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`Build failed with exit code ${code}`));
      }
    });
  });
}
