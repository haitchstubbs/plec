// scripts/browser-harness.mjs

import { accessSync, constants } from 'node:fs';
import { spawnSync } from 'node:child_process';
import path from 'node:path';

const repoRoot = path.resolve(
  import.meta.dirname,
  '..',
);

const runtimeCrate = path.join(
  repoRoot,
  'packages',
  'plec-runtime',
  'crates',
  'runtime',
);

function chromeDriverTarget() {
  if (process.platform === 'win32') {
    return {
      directory: 'chromedriver-win64',
      executable: 'chromedriver.exe',
    };
  }

  if (process.platform === 'linux') {
    if (process.arch !== 'x64') {
      throw new Error(
        `Unsupported ChromeDriver platform: linux/${process.arch}`,
      );
    }

    return {
      directory: 'chromedriver-linux64',
      executable: 'chromedriver',
    };
  }

  if (process.platform === 'darwin') {
    if (process.arch === 'arm64') {
      return {
        directory: 'chromedriver-mac-arm64',
        executable: 'chromedriver',
      };
    }

    if (process.arch === 'x64') {
      return {
        directory: 'chromedriver-mac-x64',
        executable: 'chromedriver',
      };
    }
  }

  throw new Error(
    `Unsupported ChromeDriver platform: ${process.platform}/${process.arch}`,
  );
}

function resolveChromeDriver() {
  if (process.env.CHROMEDRIVER) {
    return path.resolve(process.env.CHROMEDRIVER);
  }

  const { directory, executable } =
    chromeDriverTarget();

  return path.join(
    repoRoot,
    directory,
    executable,
  );
}

const chromeDriver = resolveChromeDriver();

try {
  accessSync(chromeDriver, constants.X_OK);
} catch {
  console.error(
    `ChromeDriver is not installed or executable:\n  ${chromeDriver}`,
  );
  console.error(
    '\nRun `yarn setup` to install the required browser tooling.',
  );

  process.exitCode = 1;
} 

if (!process.exitCode) {
  const driverDir = path.dirname(chromeDriver);

  console.log(
    `Using ChromeDriver: ${chromeDriver}`,
  );

  const result = spawnSync(
    'wasm-pack',
    [
      'test',
      '--headless',
      '--chrome',
      runtimeCrate,
      ...(process.argv.length > 2
        ? ['--', ...process.argv.slice(2)]
        : []),
    ],
    {
      cwd: repoRoot,
      stdio: 'inherit',
      env: {
        ...process.env,

        // wasm-bindgen/wasm-pack can locate the driver
        // either through CHROMEDRIVER or PATH.
        CHROMEDRIVER: chromeDriver,
        PATH: [
          driverDir,
          process.env.PATH,
        ]
          .filter(Boolean)
          .join(path.delimiter),
      },
    },
  );

  if (result.error) {
    console.error(
      `Failed to run wasm-pack: ${result.error.message}`,
    );

    process.exitCode = 1;
  } else {
    process.exitCode = result.status ?? 1;
  }
}