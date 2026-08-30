// scripts/browser-harness.mjs

import { accessSync, constants, readdirSync } from 'node:fs';
import { mkdir, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import os from 'node:os';
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

function resolveBrowserExecutable() {
  if (process.env.PLEC_CHROME_EXECUTABLE) {
    return process.env.PLEC_CHROME_EXECUTABLE;
  }
  // The setup script provisions ChromeDriver without a browser; the only
  // Chrome on this machine comes from the Playwright cache. Pick the newest
  // installed Chromium so the driver can be matched to its major version.
  const cache = path.join(
    os.homedir(),
    '.cache',
    'ms-playwright',
  );
  try {
    const newest = readdirSync(cache)
      .filter((entry) => /^chromium-\d+$/.test(entry))
      .sort()
      .pop();
    const binary = path.join(cache, newest, 'chrome-linux64', 'chrome');
    accessSync(binary, constants.X_OK);
    return binary;
  } catch {
    return undefined;
  }
}

function majorVersion(binary, versionFlag) {
  const result = spawnSync(binary, [versionFlag], { encoding: 'utf8' });
  return /(\d+)\./.exec(result.stdout ?? '')?.[1];
}

const chromeDriver = resolveChromeDriver();
const browserExecutable = resolveBrowserExecutable();
const driverExecutable = (() => {
  const browserMajor = browserExecutable
    ? majorVersion(browserExecutable, '--version')
    : undefined;
  const driverMajor = majorVersion(chromeDriver, '--version');
  if (!browserMajor || browserMajor === driverMajor) return chromeDriver;
  // setup.mjs installs the stable driver; a browser from a different release
  // channel needs its matching major from the same tooling directory.
  const matched = path.join(
    path.dirname(chromeDriver),
    `chromedriver-${browserMajor}`,
  );
  try {
    accessSync(matched, constants.X_OK);
    return matched;
  } catch {
    return chromeDriver;
  }
})();

try {
  accessSync(driverExecutable, constants.X_OK);
} catch {
  console.error(
    `ChromeDriver is not installed or executable:\n  ${driverExecutable}`,
  );
  console.error(
    '\nRun `yarn setup` to install the required browser tooling.',
  );

  process.exitCode = 1;
}

if (!process.exitCode) {
  const driverDir = path.dirname(driverExecutable);

  console.log(
    `Using ChromeDriver: ${driverExecutable}`,
  );
  if (browserExecutable) {
    console.log(`Using Chrome browser: ${browserExecutable}`);
  }

  // Chromedriver only auto-discovers its matching browser from PATH. When the
  // browser comes from the Playwright cache, point the session at it through
  // the runner's webdriver capability config (kept in the git-ignored target
  // directory).
  let webdriverJson;
  if (browserExecutable) {
    webdriverJson = path.join(repoRoot, 'target', 'webdriver.json');
    await mkdir(path.dirname(webdriverJson), { recursive: true });
    await writeFile(
      webdriverJson,
      JSON.stringify({
        'goog:chromeOptions': { binary: browserExecutable },
      }),
    );
  }

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
        CHROMEDRIVER: driverExecutable,
        ...(webdriverJson
          ? { WASM_BINDGEN_TEST_WEBDRIVER_JSON: webdriverJson }
          : {}),
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