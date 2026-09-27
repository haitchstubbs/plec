import { existsSync, readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import {
  chromeDriverPath,
  chromiumExecutable,
  expectedChromiumFromPlaywright,
  repoRoot,
  toolchain,
  versionFromOutput,
  wasmBindgenToolPath,
} from './toolchain.mjs';

const browser = process.argv.includes('--browser');
const failures = [];

function commandVersion(command, args = ['--version']) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    encoding: 'utf8',
    shell: false,
  });
  if (result.error || result.status !== 0) return undefined;
  return versionFromOutput(`${result.stdout}\n${result.stderr}`);
}

function expectVersion(label, actual, expected) {
  if (actual !== expected) {
    failures.push(
      `${label}: expected ${expected}, found ${actual ?? 'missing'}`,
    );
  }
}

function lockedPackageVersions(name) {
  const lock = readFileSync(path.join(repoRoot, 'Cargo.lock'), 'utf8');
  return [
    ...lock.matchAll(
      new RegExp(
        `\\[\\[package\\]\\]\\nname = "${name}"\\nversion = "([^"]+)"`,
        'g',
      ),
    ),
  ].map((match) => match[1]);
}

function verifyCargoLock() {
  const expected = {
    'serde-wasm-bindgen': '0.6.5',
    'wasm-bindgen': '0.2.128',
    'wasm-bindgen-futures': '0.4.78',
    'wasm-bindgen-test': '0.3.78',
    'web-sys': '0.3.105',
    'js-sys': '0.3.105',
  };

  for (const [name, version] of Object.entries(expected)) {
    const versions = lockedPackageVersions(name);
    expectVersion(
      `Cargo.lock ${name}`,
      versions.join(', ') || undefined,
      version,
    );
  }
}

function verifyCargoResolution() {
  const result = spawnSync(
    'cargo',
    ['metadata', '--locked', '--format-version', '1', '--no-deps'],
    {
      cwd: repoRoot,
      encoding: 'utf8',
      shell: false,
    },
  );
  if (result.error || result.status !== 0) {
    failures.push(
      `Cargo lockfile: ${result.stderr?.trim() || result.error?.message || 'cargo metadata failed'}`,
    );
  }
}

function packageVersion(packagePath) {
  if (!existsSync(packagePath)) return undefined;
  return JSON.parse(readFileSync(packagePath, 'utf8')).version;
}

expectVersion('Node', process.versions.node, toolchain.runtime.node);
expectVersion('Yarn', commandVersion('yarn'), toolchain.runtime.yarn);
expectVersion('Rust', commandVersion('rustc'), toolchain.runtime.rust);
expectVersion(
  'wasm-pack',
  commandVersion('wasm-pack'),
  toolchain['build-tools']['wasm-pack'],
);
expectVersion(
  'wasm-tools',
  commandVersion('wasm-tools'),
  toolchain['build-tools']['wasm-tools'],
);
expectVersion(
  'wasm-bindgen-cli',
  commandVersion(wasmBindgenToolPath('wasm-bindgen')),
  toolchain['build-tools']['wasm-bindgen-cli'],
);
expectVersion(
  'wasm-bindgen-test-runner',
  commandVersion(wasmBindgenToolPath('wasm-bindgen-test-runner')),
  toolchain['build-tools']['wasm-bindgen-cli'],
);
verifyCargoLock();
verifyCargoResolution();

if (browser) {
  const browserTools = toolchain['browser-tools'];
  expectVersion(
    '@playwright/test',
    packageVersion(
      path.join(
        repoRoot,
        'node_modules',
        '@playwright',
        'test',
        'package.json',
      ),
    ),
    browserTools.playwright,
  );
  expectVersion(
    'playwright-core',
    packageVersion(
      path.join(
        repoRoot,
        'node_modules',
        'playwright-core',
        'package.json',
      ),
    ),
    browserTools.playwright,
  );
  expectVersion(
    '@puppeteer/browsers',
    packageVersion(
      path.join(
        repoRoot,
        'node_modules',
        '@puppeteer',
        'browsers',
        'package.json',
      ),
    ),
    browserTools['puppeteer-browsers'],
  );

  try {
    const chromium = expectedChromiumFromPlaywright();
    expectVersion(
      'Playwright Chromium revision',
      chromium.revision,
      browserTools.chromium.revision,
    );
    expectVersion(
      'Playwright Chromium version',
      chromium.version,
      browserTools.chromium.version,
    );
  } catch (error) {
    failures.push(error.message);
  }

  const chromium = chromiumExecutable();
  expectVersion(
    'Chromium',
    chromium && commandVersion(chromium),
    browserTools.chromium.version,
  );
  expectVersion(
    'ChromeDriver',
    commandVersion(chromeDriverPath()),
    browserTools.chromedriver,
  );
}

if (failures.length) {
  console.error('Toolchain verification failed:');
  for (const failure of failures) console.error(`- ${failure}`);
  console.error(
    '\nRun `yarn setup` to provision the pinned toolchain.',
  );
  process.exitCode = 1;
} else {
  console.log(
    `Pinned toolchain verified${browser ? ' (including browser)' : ''}.`,
  );
}
