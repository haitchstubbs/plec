import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const appDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const sourceRoots = ['src', 'scripts'].map((directory) =>
  path.join(appDir, directory),
);
const sourceExtension = /\.(?:[cm]?[jt]sx?)$/;
const reactImport =
  /(?:import|export)\s+(?:[\s\S]*?\s+from\s+)?['"](?:react|react-dom)(?:\/[^'"]*)?['"]|(?:require|import)\s*\(\s*['"](?:react|react-dom)(?:\/[^'"]*)?['"]\s*\)/;

async function sourceFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = await Promise.all(
    entries.map(async (entry) => {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) return sourceFiles(file);
      return sourceExtension.test(entry.name) ? [file] : [];
    }),
  );
  return files.flat();
}

const manifest = JSON.parse(
  await readFile(path.join(appDir, 'package.json'), 'utf8'),
);
const reactDependencies = [
  'dependencies',
  'devDependencies',
  'peerDependencies',
  'optionalDependencies',
].flatMap((field) =>
  Object.keys(manifest[field] ?? {}).filter(
    (name) =>
      name === 'react' ||
      name === 'react-dom' ||
      name.startsWith('react/'),
  ),
);

const invalidImports = [];
for (const file of (
  await Promise.all(sourceRoots.map(sourceFiles))
).flat()) {
  if (reactImport.test(await readFile(file, 'utf8')))
    invalidImports.push(path.relative(appDir, file));
}

const client = await readFile(path.join(appDir, 'src', 'client.tsx'), 'utf8');
const invalidClientBoundary =
  !client.includes('startPlecRouter(') ||
  /from\s+['"]\.\/routes|\bfetch\s*\(|\/api\//.test(client);

if (reactDependencies.length || invalidImports.length || invalidClientBoundary) {
  const violations = [
    reactDependencies.length &&
      `React dependencies: ${reactDependencies.join(', ')}`,
    invalidImports.length &&
      `React imports: ${invalidImports.join(', ')}`,
    invalidClientBoundary &&
      'Browser entry must only bootstrap the Plec runtime with artifact URLs',
  ]
    .filter(Boolean)
    .join('; ');
  throw new Error(
    `apps/fullstack must remain React-free. ${violations}. Use Plec instead.`,
  );
}
