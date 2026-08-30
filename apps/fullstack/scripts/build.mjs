import { copyFile, mkdir, readdir } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const appDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);

const require = createRequire(import.meta.url);

const assetsDir = path.join(appDir, 'dist', 'public', 'assets');

//
// Plec build (framework-owned pipeline)
//

execFileSync(
  'plec',
  [
    'build',
    'src/router.tsx',
    '--out-dir',
    'dist',
    '--title',
    'Plec fullstack playground',
  ],
  {
    cwd: appDir,
    stdio: 'inherit',
  },
);

//
// Tailwind (application-owned)
//

const tailwindCli = path.join(
  path.dirname(require.resolve('@tailwindcss/cli/package.json')),
  'dist',
  'index.mjs',
);

execFileSync(
  process.execPath,
  [
    tailwindCli,
    '-i',
    'src/styles.css',
    '-o',
    'dist/public/assets/styles.css',
  ],
  {
    cwd: appDir,
    stdio: 'inherit',
  },
);

//
// Fonts (application-owned)
//

await Promise.all([
  copyFontFiles('@fontsource-variable/outfit'),
  copyFontFiles('@fontsource-variable/raleway'),
]);

/**
 * Tailwind preserves Fontsource's relative
 * `./files/*.woff2` URLs.
 */
async function copyFontFiles(packageName) {
  const sourceDir = path.join(
    path.dirname(require.resolve(`${packageName}/package.json`)),
    'files',
  );

  const destinationDir = path.join(assetsDir, 'files');

  await mkdir(destinationDir, {
    recursive: true,
  });

  for (const file of await readdir(sourceDir)) {
    if (!file.endsWith('.woff2')) {
      continue;
    }

    await copyFile(
      path.join(sourceDir, file),
      path.join(destinationDir, file),
    );
  }
}
