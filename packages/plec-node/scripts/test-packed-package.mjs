import { execFileSync } from 'node:child_process';
import { mkdtemp, readFile, rm, stat } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const packageRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const packageJson = JSON.parse(
  await readFile(path.join(packageRoot, 'package.json'), 'utf8'),
);
const supportedTargets = [
  { name: 'linux-x64-gnu', os: 'linux', cpu: 'x64', libc: 'glibc' },
  { name: 'linux-x64-musl', os: 'linux', cpu: 'x64', libc: 'musl' },
  { name: 'linux-arm64-gnu', os: 'linux', cpu: 'arm64', libc: 'glibc' },
  { name: 'darwin-arm64', os: 'darwin', cpu: 'arm64' },
  { name: 'darwin-x64', os: 'darwin', cpu: 'x64' },
  { name: 'win32-x64-msvc', os: 'win32', cpu: 'x64' },
];
for (const target of supportedTargets) {
  const targetPackageName = `${packageJson.name}-${target.name}`;
  const targetPackage = JSON.parse(
    await readFile(
      path.join(packageRoot, 'npm', target.name, 'package.json'),
      'utf8',
    ),
  );
  if (targetPackage.name !== targetPackageName)
    throw new Error(
      `expected ${targetPackageName}, found ${targetPackage.name}`,
    );
  if (targetPackage.version !== packageJson.version)
    throw new Error(
      `${targetPackageName} version differs from the wrapper`,
    );
  if (
    packageJson.optionalDependencies?.[targetPackageName] !==
    packageJson.version
  )
    throw new Error(
      `${targetPackageName} is missing from matching optionalDependencies`,
    );
  if (targetPackage.main !== `index.${target.name}.node`)
    throw new Error(
      `${targetPackageName} has an unexpected native entry`,
    );
  if (
    JSON.stringify(targetPackage.os) !== JSON.stringify([target.os]) ||
    JSON.stringify(targetPackage.cpu) !== JSON.stringify([target.cpu])
  )
    throw new Error(
      `${targetPackageName} has incorrect OS/CPU constraints`,
    );
  if (
    target.libc &&
    JSON.stringify(targetPackage.libc) !== JSON.stringify([target.libc])
  )
    throw new Error(
      `${targetPackageName} has incorrect libc constraints`,
    );
}
const target = currentNativeTarget();
const platformPackageName = `${packageJson.name}-${target}`;
const platformPackageDir = path.join(packageRoot, 'npm', target);
const platformPackage = JSON.parse(
  await readFile(path.join(platformPackageDir, 'package.json'), 'utf8'),
);
if (platformPackage.name !== platformPackageName)
  throw new Error(
    `expected ${platformPackageName}, found ${platformPackage.name}`,
  );
if (platformPackage.version !== packageJson.version)
  throw new Error('root and platform package versions differ');
if (!packageJson.optionalDependencies?.[platformPackageName])
  throw new Error(
    `${platformPackageName} is missing from optionalDependencies`,
  );
if (
  packageJson.optionalDependencies[platformPackageName] !==
  packageJson.version
)
  throw new Error(
    `${platformPackageName} optional dependency version differs`,
  );
await stat(path.join(platformPackageDir, platformPackage.main));

const temporaryDirectory = await mkdtemp(
  path.join(os.tmpdir(), 'plec-node-package-test-'),
);
try {
  const rootTarball = pack(packageRoot, temporaryDirectory);
  const platformTarball = pack(platformPackageDir, temporaryDirectory);
  const installation = path.join(temporaryDirectory, 'installation');
  const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
  execFileSync(
    npm,
    [
      'install',
      '--prefix',
      installation,
      '--ignore-scripts',
      '--no-audit',
      '--no-fund',
      '--omit=optional',
      rootTarball,
      platformTarball,
    ],
    { stdio: 'inherit' },
  );
  execFileSync(
    process.execPath,
    [
      '--input-type=module',
      '-e',
      `import { createPlecHandler, serve } from '@plec/node'; if (typeof createPlecHandler !== 'function' || typeof serve !== 'function') process.exit(1);`,
    ],
    { cwd: installation, stdio: 'inherit' },
  );
} finally {
  await rm(temporaryDirectory, { recursive: true, force: true });
}

console.info(
  `Packed @plec/node and ${platformPackageName} load successfully`,
);

function pack(directory, destination) {
  const output = execFileSync(
    process.platform === 'win32' ? 'npm.cmd' : 'npm',
    ['pack', directory, '--pack-destination', destination, '--json'],
    { encoding: 'utf8' },
  );
  const [result] = JSON.parse(output);
  if (!result?.filename)
    throw new Error(`npm pack failed for ${directory}`);
  return path.join(destination, result.filename);
}

function currentNativeTarget() {
  if (process.platform === 'linux') {
    const report = process.report?.getReport();
    const libc = report?.header?.glibcVersionRuntime ? 'gnu' : 'musl';
    const arch = process.arch === 'x64' ? 'x64' : process.arch;
    return `linux-${arch}-${libc}`;
  }
  if (process.platform === 'darwin') return `darwin-${process.arch}`;
  if (process.platform === 'win32') return `win32-${process.arch}-msvc`;
  throw new Error(
    `package smoke test is not configured for ${process.platform}`,
  );
}
