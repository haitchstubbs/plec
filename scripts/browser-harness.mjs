// scripts/browser-harness.mjs

import { accessSync, constants } from 'node:fs';
import { mkdir, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { findPlaywrightChromium } from './utils.js';

const repoRoot = path.resolve(import.meta.dirname, '..');

const runtimeCrate = path.join(repoRoot, 'crates', 'plec-runtime');

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

  const { directory, executable } = chromeDriverTarget();

  return path.join(repoRoot, '.tools', directory, executable);
}

function resolveBrowserExecutable() {
  // The setup script provisions ChromeDriver to match the Playwright-managed
  // Chromium; reuse the same lookup so harness and setup agree on the browser.
  return findPlaywrightChromium();
}

function majorVersion(binary, versionFlag) {
  const result = spawnSync(binary, [versionFlag], { encoding: 'utf8' });
  return /(\d+)\./.exec(result.stdout ?? '')?.[1];
}

const chromeDriver = resolveChromeDriver();
const browserExecutable = resolveBrowserExecutable();

try {
  accessSync(chromeDriver, constants.X_OK);
} catch {
  console.error(
    `ChromeDriver is not installed or executable:\n  ${chromeDriver}`,
  );
  console.error(
    '\nRun `yarn install:build-tools` to install the required browser tooling.',
  );

  process.exitCode = 1;
}

if (!process.exitCode) {
  const driverMajor = majorVersion(chromeDriver, '--version');
  const browserMajor = browserExecutable
    ? majorVersion(browserExecutable, '--version')
    : undefined;

  // A mismatched driver fails deep inside wasm-pack with an opaque session
  // error; fail here with the versions and the fix instead.
  if (driverMajor && browserMajor && driverMajor !== browserMajor) {
    console.error(
      `ChromeDriver ${driverMajor}.x does not match Chrome ${browserMajor}.x:\n` +
        `  driver:  ${chromeDriver}\n` +
        `  browser: ${browserExecutable}\n` +
        `\nRun \`yarn install:build-tools\` to provision a matching ChromeDriver,\n` +
        `or point CHROMEDRIVER at a ChromeDriver built for Chrome ${browserMajor}.`,
    );

    process.exitCode = 1;
  }
}

if (!process.exitCode) {
  const driverDir = path.dirname(chromeDriver);

  console.log(`Using ChromeDriver: ${chromeDriver}`);
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
        CHROMEDRIVER: chromeDriver,
        ...(webdriverJson
          ? { WASM_BINDGEN_TEST_WEBDRIVER_JSON: webdriverJson }
          : {}),
        PATH: [driverDir, process.env.PATH]
          .filter(Boolean)
          .join(path.delimiter),
      },
    },
  );

  if (result.error) {
    console.error(`Failed to run wasm-pack: ${result.error.message}`);

    process.exitCode = 1;
  } else {
    process.exitCode = result.status ?? 1;
  }
}
