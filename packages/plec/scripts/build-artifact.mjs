#!/usr/bin/env node
/**
 * Assembles the `plec` release artifact: one self-contained package folder
 * consumable without the monorepo. The artifact is the package folder itself;
 * this script fills `dist/` with everything it must carry:
 *
 * - compiled JS (tsc) plus bundled `./server` / `./browser` entries
 *   (the browser package's built output is inlined so no bare workspace
 *   specifier survives)
 * - validated WASM runtime assets published into dist/runtime
 * - the cargo-built release-variant CLI binary (PLEC_CLI_VERSION=release,
 *   the public-build variant per `.env.plec` semantics)
 *
 * A final audit fails the build if the folder still references the
 * monorepo (workspace protocol, bare workspace-package specifiers) or if a
 * browser entry can pull Node builtins into a client bundle.
 */
import fs from 'node:fs';
import path from 'node:path';
import {
  buildWorkspaceSurface,
  distDir,
  packageDir,
  repoRoot,
  run,
} from './build-package.mjs';
const isWindows = process.platform === 'win32';

// ---------------------------------------------------------------------------
// Product-version consistency gate: the release artifact's public metadata
// (package.json) and the compiled CLI binary describe the same Plec release,
// so a Cargo/package.json drift must fail before anything is assembled.
// ---------------------------------------------------------------------------
function readWorkspaceVersion() {
  const cargo = fs.readFileSync(
    path.join(repoRoot, 'Cargo.toml'),
    'utf8',
  );
  let inWorkspacePackage = false;
  for (const line of cargo.split('\n')) {
    const section = /^\s*\[([^\]]+)\]/.exec(line);
    if (section) {
      inWorkspacePackage = section[1] === 'workspace.package';
      continue;
    }
    if (!inWorkspacePackage) continue;
    const match = /^\s*version\s*=\s*"([^"]+)"\s*$/.exec(line);
    if (match) return match[1];
  }
  throw new Error(
    'cannot find [workspace.package] version in Cargo.toml',
  );
}

const cargoVersion = readWorkspaceVersion();
const packageVersion = JSON.parse(
  fs.readFileSync(path.join(packageDir, 'package.json'), 'utf8'),
).version;
if (cargoVersion !== packageVersion) {
  console.error(
    `Plec product version mismatch: Cargo workspace declares ${cargoVersion}, ` +
      `packages/plec declares ${packageVersion}. ` +
      'Realign with `plec workspace version --set <version>`.',
  );
  process.exit(1);
}

await buildWorkspaceSurface();

console.log(
  'Building release CLI binary (PLEC_CLI_VERSION=release)...',
);
run('cargo', ['build', '--release', '-p', 'plec-cli'], {
  env: { ...process.env, PLEC_CLI_VERSION: 'release' },
});
const binaryName = isWindows ? 'plec.exe' : 'plec';
const binDir = path.join(distDir, 'bin');
fs.mkdirSync(binDir, { recursive: true });
fs.copyFileSync(
  path.join(repoRoot, 'target/release', binaryName),
  path.join(binDir, binaryName),
);
if (!isWindows) fs.chmodSync(path.join(binDir, binaryName), 0o755);

// ---------------------------------------------------------------------------
// Self-containment audit: the artifact must be inspectable as a folder alone.
// ---------------------------------------------------------------------------
const workspaceSpecifier = /(?:from|import)\s*['"]plec-browser['"]/;
const failures = [];
for (const entry of walk(distDir)) {
  // Test files ship in dist by existing convention; they are not runtime
  // surface and quote the audited patterns themselves.
  if (/\.test\.(js|mjs|cjs)$/.test(entry)) continue;
  if (!/\.(js|mjs|cjs)$/.test(entry)) continue;
  const source = fs.readFileSync(entry, 'utf8');
  if (workspaceSpecifier.test(source))
    failures.push(
      `${relative(entry)} references a workspace package specifier`,
    );
  if (/workspace:/.test(source))
    failures.push(
      `${relative(entry)} contains a workspace: protocol reference`,
    );
  if (
    entry.endsWith('browser.js') &&
    /(?:from|import)\s*['"]node:/.test(source)
  )
    failures.push(`${relative(entry)} imports Node builtins`);
}

for (const required of [
  'runtime/runtime_bg.wasm',
  'runtime/runtime_bg.wasm.br',
  'runtime/runtime.js',
  'runtime/runtime.js.br',
  'runtime/provenance.json',
  'node-runtime.mjs',
  `bin/${binaryName}`,
]) {
  if (!fs.existsSync(path.join(distDir, required)))
    failures.push(`missing artifact asset: dist/${required}`);
}

if (failures.length > 0) {
  console.error('Artifact self-containment audit failed:');
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}

console.log(`Artifact assembled: ${packageDir} (dist/, bin/)`);

function relative(filePath) {
  return path.relative(packageDir, filePath);
}

function* walk(directory) {
  for (const item of fs.readdirSync(directory, {
    withFileTypes: true,
  })) {
    const entryPath = path.join(directory, item.name);
    if (item.isDirectory()) yield* walk(entryPath);
    else yield entryPath;
  }
}
