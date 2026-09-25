import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
} from 'node:fs';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import cliToolsConfig from '../cli-tools.json' with { type: 'json' };
import {
  chromeDriverPath,
  chromeDriverTarget,
  chromiumExecutable,
  expectedChromiumFromPlaywright,
  playwrightBrowsersPath,
  toolchain,
  versionFromOutput,
  wasmBindgenToolPath,
  wasmBindgenToolRoot,
} from './toolchain.mjs';

const { 'build-tools': buildTools } = cliToolsConfig;

const scriptsDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptsDir, '..');

function cargoBinPath(name) {
  return path.join(
    process.env.CARGO_HOME ?? path.join(process.env.HOME, '.cargo'),
    'bin',
    `${name}${process.platform === 'win32' ? '.exe' : ''}`,
  );
}

function run(command, args, options = {}) {
  return spawnSync(command, args, {
    stdio: 'inherit',
    shell: false,
    ...options,
  });
}

function prebuiltTool(name) {
  return cliToolsConfig['prebuilt-build-tools']?.[
    `${process.platform}-${process.arch}`
  ]?.[name];
}

function releaseAssetUrl(name, version, target) {
  const asset = {
    'wasm-pack': {
      repository: 'wasm-bindgen/wasm-pack',
      tag: `v${version}`,
      filename: `wasm-pack-v${version}-${target}.tar.gz`,
    },
    'wasm-tools': {
      repository: 'bytecodealliance/wasm-tools',
      tag: `v${version}`,
      filename: `wasm-tools-${version}-${target}.tar.gz`,
    },
    'wasm-bindgen-cli': {
      repository: 'wasm-bindgen/wasm-bindgen',
      tag: version,
      filename: `wasm-bindgen-${version}-${target}.tar.gz`,
    },
  }[name];

  return `https://github.com/${asset.repository}/releases/download/${asset.tag}/${asset.filename}`;
}

function installPrebuiltTool({ name, version, root }) {
  const asset = prebuiltTool(name);
  if (!asset) {
    if (process.env.CI) {
      throw new Error(
        `No pinned prebuilt ${name} release is configured for ${process.platform}/${process.arch}.`,
      );
    }
    return false;
  }

  const archive = path.join(
    repoRoot,
    '.cache',
    `${name}-${version}.tar.gz`,
  );
  mkdirSync(path.dirname(archive), { recursive: true });
  mkdirSync(root, { recursive: true });

  console.log(`Downloading pinned ${name} ${version} release...`);
  const download = run('curl', [
    '--fail',
    '--location',
    '--silent',
    '--show-error',
    '--output',
    archive,
    releaseAssetUrl(name, version, asset.target),
  ]);
  if (download.error || download.status !== 0) {
    throw new Error(`Failed to download pinned ${name} ${version}.`);
  }

  const actual = createHash('sha256')
    .update(readFileSync(archive))
    .digest('hex');
  if (actual !== asset.sha256) {
    rmSync(archive, { force: true });
    throw new Error(
      `Pinned ${name} checksum mismatch: expected ${asset.sha256}, found ${actual}.`,
    );
  }

  const extract = run('tar', [
    '--extract',
    '--gzip',
    '--file',
    archive,
    '--strip-components=1',
    '--directory',
    root,
  ]);
  rmSync(archive, { force: true });
  if (extract.error || extract.status !== 0) {
    throw new Error(`Failed to extract pinned ${name} ${version}.`);
  }

  return true;
}

function ensureTool({ name, version }) {
  const check = spawnSync(name, ['--version'], {
    encoding: 'utf8',
    shell: false,
  });

  if (
    !check.error &&
    check.status === 0 &&
    versionFromOutput(check.stdout ?? '') === version
  ) {
    console.log(`${name} found: ${check.stdout.trim()}`);
    return;
  }

  if (
    installPrebuiltTool({
      name,
      version,
      root: path.dirname(cargoBinPath(name)),
    })
  ) {
    console.log(`${name} ${version} installed from a pinned release.`);
    return;
  }

  console.log(`Installing pinned ${name} ${version} from source...`);

  const install = run('cargo', [
    'install',
    name,
    '--locked',
    '--version',
    version,
    '--force',
  ]);

  if (install.error) {
    console.error(
      `Failed to install ${name}: ${install.error.message}`,
    );
    process.exitCode = 1;
    return;
  }

  if (install.status !== 0) {
    process.exitCode = install.status ?? 1;
    return;
  }

  console.log(`${name} ${version} installed.`);
}

function ensureWasmBindgenCli() {
  const version = buildTools['wasm-bindgen-cli'];
  const bindgen = wasmBindgenToolPath('wasm-bindgen');
  const testRunner = wasmBindgenToolPath('wasm-bindgen-test-runner');

  if (
    existsSync(bindgen) &&
    existsSync(testRunner) &&
    versionOf(bindgen) === version &&
    versionOf(testRunner) === version
  ) {
    console.log(`wasm-bindgen-cli found: ${version}`);
    return;
  }

  rmSync(wasmBindgenToolRoot(), { recursive: true, force: true });
  if (
    installPrebuiltTool({
      name: 'wasm-bindgen-cli',
      version,
      root: path.join(wasmBindgenToolRoot(), 'bin'),
    })
  ) {
    console.log(
      `wasm-bindgen-cli ${version} installed from a pinned release.`,
    );
    return;
  }

  console.log(
    `Installing pinned wasm-bindgen-cli ${version} locally from source...`,
  );
  const install = run('cargo', [
    'install',
    'wasm-bindgen-cli',
    '--locked',
    '--version',
    version,
    '--root',
    wasmBindgenToolRoot(),
    '--force',
  ]);

  if (install.error || install.status !== 0) {
    process.exitCode = install.status ?? 1;
    return;
  }
}

function findFile(root, filename) {
  if (!existsSync(root)) {
    return undefined;
  }

  for (const entry of readdirSync(root, {
    withFileTypes: true,
  })) {
    const entryPath = path.join(root, entry.name);

    if (entry.isDirectory()) {
      const found = findFile(entryPath, filename);

      if (found) {
        return found;
      }

      continue;
    }

    if (entry.name === filename) {
      return entryPath;
    }
  }

  return undefined;
}

function ensureExecutable(
  command,
  { args = ['--version'], installHint } = {},
) {
  const check = spawnSync(command, args, {
    encoding: 'utf8',
    shell: false,
  });

  if (!check.error && check.status === 0) {
    console.log(`${command} found`);
    return true;
  }

  if (check.error) {
    console.error(`${command} check failed: ${check.error.message}`);
  } else {
    console.error(
      `${command} check exited with status ${check.status}`,
    );
  }

  if (installHint) {
    console.error(`Install with: ${installHint}`);
  }

  return false;
}

function versionOf(binary) {
  const result = spawnSync(binary, ['--version'], {
    encoding: 'utf8',
    shell: false,
  });
  return versionFromOutput(result.stdout ?? '');
}

function ensureChromiumInstalled() {
  if (chromiumExecutable()) {
    return;
  }

  if (
    !existsSync(path.join(repoRoot, 'node_modules', 'playwright-core'))
  ) {
    throw new Error(
      'Playwright is not installed. Run `yarn install --immutable` before provisioning browser tools.',
    );
  }

  const command = process.platform === 'win32' ? 'yarn.cmd' : 'yarn';
  console.log('Installing Playwright Chromium...');
  const install = run(
    command,
    [
      'workspace',
      '@plec/e2e',
      'exec',
      'playwright',
      'install',
      'chromium',
    ],
    {
      cwd: repoRoot,
      env: {
        ...process.env,
        PLAYWRIGHT_BROWSERS_PATH: playwrightBrowsersPath,
      },
    },
  );

  if (install.error || install.status !== 0) {
    throw new Error(
      'Failed to install the pinned Playwright Chromium.',
    );
  }
}

function ensureChromeDriver() {
  if (
    process.platform === 'linux' &&
    !ensureExecutable('unzip', {
      args: ['-v'],
      installHint:
        'sudo apt-get update && sudo apt-get install -y unzip',
    })
  ) {
    process.exitCode = 1;
    return;
  }
  ensureChromiumInstalled();

  const chromium = chromiumExecutable();
  const chromiumVersion = chromium ? versionOf(chromium) : undefined;
  const expectedChromium = expectedChromiumFromPlaywright();
  const expectedDriver = toolchain['browser-tools'].chromedriver;
  if (
    expectedChromium.revision !==
      toolchain['browser-tools'].chromium.revision ||
    expectedChromium.version !==
      toolchain['browser-tools'].chromium.version ||
    chromiumVersion !== expectedChromium.version
  ) {
    throw new Error(
      `Pinned Chromium mismatch: expected ${expectedChromium.version} (revision ${expectedChromium.revision}), found ${chromiumVersion ?? 'missing'}.`,
    );
  }

  const target = chromeDriverPath();
  const { directory, executable } = chromeDriverTarget();
  const platform = directory.replace('chromedriver-', '');
  const installDir = path.dirname(target);
  const driverVersion = existsSync(target)
    ? versionOf(target)
    : undefined;

  if (driverVersion === expectedDriver) {
    console.log(
      `chromedriver found: ChromeDriver ${driverVersion} (matches Chromium ${chromiumVersion})`,
    );
    return;
  }

  if (driverVersion) {
    console.log(
      `ChromeDriver ${driverVersion} does not match pinned ${expectedDriver}; reinstalling...`,
    );
  } else {
    console.log(
      `chromedriver-${platform} not found; installing ChromeDriver...`,
    );
  }

  const cacheDir = path.join(repoRoot, '.cache', 'chromedriver');

  rmSync(cacheDir, {
    recursive: true,
    force: true,
  });

  mkdirSync(cacheDir, {
    recursive: true,
  });

  const command = process.platform === 'win32' ? 'yarn.cmd' : 'yarn';

  const install = run(
    command,
    [
      'exec',
      'browsers',
      'install',
      `chromedriver@${expectedDriver}`,
      '--path',
      cacheDir,
    ],
    { cwd: repoRoot },
  );

  if (install.error || install.status !== 0) {
    console.error(`Failed to install ChromeDriver for ${platform}.`);

    process.exitCode = install.status ?? 1;
    return;
  }

  const downloaded = findFile(cacheDir, executable);

  if (!downloaded) {
    console.error(
      `ChromeDriver installed but ${executable} could not be located.`,
    );

    process.exitCode = 1;
    return;
  }

  mkdirSync(installDir, {
    recursive: true,
  });

  copyFileSync(downloaded, target);

  if (process.platform !== 'win32') {
    chmodSync(target, 0o755);
  }

  rmSync(cacheDir, {
    recursive: true,
    force: true,
  });

  const check = spawnSync(target, ['--version'], {
    encoding: 'utf8',
    shell: false,
  });

  if (check.error || check.status !== 0) {
    console.error(
      `ChromeDriver was installed but could not be executed: ${target}`,
    );

    process.exitCode = 1;
    return;
  }

  console.log(`chromedriver installed: ${check.stdout.trim()}`);
}

// Ensure required Rust build tools are installed

for (const [name, version] of Object.entries(buildTools)) {
  if (name === 'wasm-bindgen-cli') {
    ensureWasmBindgenCli();
  } else {
    ensureTool({ name, version });
  }

  if (process.exitCode) {
    break;
  }
}

if (!process.exitCode) {
  console.log('Ensuring wasm32-unknown-unknown target is installed...');

  const target = run('rustup', [
    'target',
    'add',
    'wasm32-unknown-unknown',
    '--toolchain',
    toolchain.runtime.rust,
  ]);

  if (target.error || target.status !== 0) {
    console.error('Failed to ensure wasm32-unknown-unknown target.');

    process.exitCode = target.status ?? 1;
  }
}

if (!process.exitCode) {
  ensureChromeDriver();
}

if (!process.exitCode) {
  const verify = run(
    process.execPath,
    ['scripts/verify-toolchain.mjs', '--browser'],
    {
      cwd: repoRoot,
    },
  );
  if (verify.error || verify.status !== 0) {
    process.exitCode = verify.status ?? 1;
  }
}

if (!process.exitCode) {
  console.log('WASM/browser toolchain ready.');
}
