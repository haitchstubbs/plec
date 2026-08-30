import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  rmSync,
} from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import cliToolsConfig from '../cli-tools.json' with { type: 'json' };

const { 'build-tools': buildTools } = cliToolsConfig;

const scriptsDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptsDir, '..');

function run(command, args, options = {}) {
  return spawnSync(command, args, {
    stdio: 'inherit',
    shell: false,
    ...options,
  });
}

function ensureTool({ name, version }) {
  const check = spawnSync(name, ['--version'], {
    encoding: 'utf8',
    shell: false,
  });

  if (!check.error && check.status === 0) {
    console.log(`${name} found: ${check.stdout.trim()}`);
    return;
  }

  console.log(`${name} not found; installing ${name} ${version}...`);

  const install = run('cargo', [
    'install',
    name,
    '--locked',
    '--version',
    version,
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

function chromeDriverPlatform() {
  if (process.platform === 'win32') {
    return 'win64';
  }

  if (process.platform === 'linux') {
    return process.arch === 'arm64'
      ? 'linux-arm64'
      : 'linux64';
  }

  if (process.platform === 'darwin') {
    return process.arch === 'arm64'
      ? 'mac-arm64'
      : 'mac-x64';
  }

  throw new Error(
    `Unsupported ChromeDriver platform: ${process.platform}/${process.arch}`,
  );
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
  {
    args = ['--version'],
    installHint,
  } = {},
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
    console.error(
      `${command} check failed: ${check.error.message}`,
    );
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
  const platform = chromeDriverPlatform();
  const executable =
    process.platform === 'win32'
      ? 'chromedriver.exe'
      : 'chromedriver';

  const installDir = path.join(
    repoRoot,
    `chromedriver-${platform}`,
  );

  const target = path.join(installDir, executable);

  if (existsSync(target)) {
    const check = spawnSync(target, ['--version'], {
      encoding: 'utf8',
      shell: false,
    });

    if (check.error?.code === 'EACCES') {
      console.error(
        `Cannot execute ${target}: ${check.error.message}`,
      );
    } else if (check.error?.code === 'ENOENT') {
      console.error(`${target} was not found.`);
    } else if (check.error) {
      console.error(
        `Failed to check ${target}: ${check.error.message}`,
      );
    }
  
    if (!check.error && check.status === 0) {
      console.log(
        `chromedriver found: ${check.stdout.trim()}`,
      );

      return;
    }
  }

  console.log(
    `chromedriver-${platform} not found; installing stable ChromeDriver...`,
  );

  const cacheDir = path.join(
    repoRoot,
    '.cache',
    'chromedriver',
  );

  rmSync(cacheDir, {
    recursive: true,
    force: true,
  });

  mkdirSync(cacheDir, {
    recursive: true,
  });

  const command =
    process.platform === 'win32'
      ? 'npx.cmd'
      : 'npx';

  const install = run(command, [
    '--yes',
    '@puppeteer/browsers',
    'install',
    'chromedriver@stable',
    '--path',
    cacheDir,
  ]);

  if (install.error || install.status !== 0) {
    console.error(
      `Failed to install ChromeDriver for ${platform}.`,
    );

    process.exitCode = install.status ?? 1;
    return;
  }

  const downloaded = findFile(
    cacheDir,
    executable,
  );

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

  console.log(
    `chromedriver installed: ${check.stdout.trim()}`,
  );
}

// Ensure required Rust build tools are installed

for (const [name, version] of Object.entries(buildTools)) {
  ensureTool({ name, version });

  if (process.exitCode) {
    break;
  }
}

if (!process.exitCode) {
  console.log(
    'Ensuring wasm32-unknown-unknown target is installed...',
  );

  const target = run('rustup', [
    'target',
    'add',
    'wasm32-unknown-unknown',
  ]);

  if (target.error || target.status !== 0) {
    console.error(
      'Failed to ensure wasm32-unknown-unknown target.',
    );

    process.exitCode = target.status ?? 1;
  }
}

if (!process.exitCode) {
  ensureChromeDriver();
}

if (!process.exitCode) {
  console.log('WASM/browser toolchain ready.');
}