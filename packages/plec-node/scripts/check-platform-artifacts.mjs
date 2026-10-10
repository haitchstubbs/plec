import { access, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const packageRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const packageJson = JSON.parse(
  await readFile(path.join(packageRoot, 'package.json'), 'utf8'),
);
const targets = [
  'linux-x64-gnu',
  'linux-x64-musl',
  'linux-arm64-gnu',
  'darwin-arm64',
  'darwin-x64',
  'win32-x64-msvc',
];

for (const target of targets) {
  const packageName = `${packageJson.name}-${target}`;
  const packageDirectory = path.join(packageRoot, 'npm', target);
  const platform = JSON.parse(
    await readFile(path.join(packageDirectory, 'package.json'), 'utf8'),
  );
  if (platform.version !== packageJson.version)
    throw new Error(`${packageName} version differs from its wrapper`);
  if (
    packageJson.optionalDependencies?.[packageName] !==
    packageJson.version
  )
    throw new Error(
      `${packageName} is not an optional dependency at matching version`,
    );
  await access(path.join(packageDirectory, platform.main));
}

console.info(
  `All ${targets.length} @plec/node platform artifacts are staged`,
);
