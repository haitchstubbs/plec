import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import cliToolsConfig from '../cli-tools.json' with { type: 'json' };

export const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
export const toolchain = cliToolsConfig;
export const playwrightBrowsersPath = path.join(
  repoRoot,
  '.cache',
  'ms-playwright',
);
export const wasmPackCachePath = path.join(
  repoRoot,
  '.cache',
  'wasm-pack',
);

export function wasmBindgenToolRoot() {
  return path.join(
    repoRoot,
    '.tools',
    `wasm-bindgen-${toolchain['build-tools']['wasm-bindgen-cli']}`,
  );
}

export function wasmBindgenToolPath(binary) {
  return path.join(
    wasmBindgenToolRoot(),
    'bin',
    `${binary}${process.platform === 'win32' ? '.exe' : ''}`,
  );
}

export function wasmBindgenBinPath() {
  return path.dirname(wasmBindgenToolPath('wasm-bindgen'));
}

export function chromiumExecutable() {
  const { revision } = toolchain['browser-tools'].chromium;
  const root = path.join(
    playwrightBrowsersPath,
    `chromium-${revision}`,
  );
  const layouts = {
    win32: [
      ['chrome-win64', 'chrome.exe'],
      ['chrome-win', 'chrome.exe'],
    ],
    darwin: [
      ['chrome-mac', 'Chromium.app', 'Contents', 'MacOS', 'Chromium'],
    ],
  }[process.platform] ?? [
    ['chrome-linux64', 'chrome'],
    ['chrome-linux', 'chrome'],
  ];

  for (const segments of layouts) {
    const executable = path.join(root, ...segments);
    if (existsSync(executable)) return executable;
  }

  return undefined;
}

export function chromeDriverTarget() {
  if (process.platform === 'win32') {
    return {
      directory: 'chromedriver-win64',
      executable: 'chromedriver.exe',
    };
  }

  if (process.platform === 'linux') {
    if (!['x64', 'arm64'].includes(process.arch)) {
      throw new Error(
        `Unsupported ChromeDriver platform: linux/${process.arch}`,
      );
    }
    return {
      directory:
        process.arch === 'arm64'
          ? 'chromedriver-linux-arm64'
          : 'chromedriver-linux64',
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

export function chromeDriverPath() {
  const { directory, executable } = chromeDriverTarget();
  return path.join(repoRoot, '.tools', directory, executable);
}

export function versionFromOutput(output) {
  return /(\d+\.\d+\.\d+(?:\.\d+)?)/.exec(output)?.[1];
}

export function expectedChromiumFromPlaywright() {
  const manifestPath = path.join(
    repoRoot,
    'node_modules',
    'playwright-core',
    'browsers.json',
  );
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  const chromium = manifest.browsers.find(
    ({ name }) => name === 'chromium',
  );

  if (!chromium) {
    throw new Error(`Chromium is not declared in ${manifestPath}`);
  }

  return {
    revision: String(chromium.revision),
    version: chromium.browserVersion,
  };
}
