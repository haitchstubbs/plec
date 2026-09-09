import { mkdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as lucide from 'lucide';

const icons = Object.entries(lucide)
  .filter(([, value]) => Array.isArray(value))
  .filter(([name]) => /^[A-Z][A-Za-z0-9]*$/.test(name))
  .sort(([left], [right]) => left.localeCompare(right));

const packageDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);

const sourceDir = path.join(packageDir, 'src/icons');

const filenameFor = (name) =>
  name.replace(/([a-z0-9])([A-Z])/g, '$1-$2').toLowerCase();

await rm(sourceDir, { recursive: true, force: true });

await rm(path.join(packageDir, 'src/icons.generated.ts'), {
  force: true,
});

await mkdir(sourceDir, {
  recursive: true,
});

for (const [name] of icons) {
  const filename = filenameFor(name);

  await writeFile(
    path.join(sourceDir, `${filename}.tsx`),
    [
      '// Generated from the installed lucide package. Do not edit by hand.',
      '',
      `export { ${name} } from 'lucide';`,
      '',
    ].join('\n'),
  );
}
