import { mkdir, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import {
  chromeDriverPath,
  chromiumExecutable,
  playwrightBrowsersPath,
  repoRoot,
  wasmBindgenBinPath,
  wasmPackCachePath,
} from '../../../scripts/toolchain.mjs';

const verify = spawnSync(
  process.execPath,
  ['scripts/verify-toolchain.mjs', '--browser'],
  { cwd: repoRoot, stdio: 'inherit' },
);

if (verify.status !== 0) process.exit(verify.status ?? 1);

const runtimeCrate = path.join(repoRoot, 'crates', 'plec-runtime');
const chromeDriver = chromeDriverPath();
const browserExecutable = chromiumExecutable();
const driverDir = path.dirname(chromeDriver);
const webdriverJson = path.join(repoRoot, 'target', 'webdriver.json');

await mkdir(path.dirname(webdriverJson), { recursive: true });
await writeFile(
  webdriverJson,
  JSON.stringify({
    'goog:chromeOptions': { binary: browserExecutable },
  }),
);

console.log(`Using ChromeDriver: ${chromeDriver}`);
console.log(`Using Chrome browser: ${browserExecutable}`);

const result = spawnSync(
  'wasm-pack',
  [
    'test',
    '--mode',
    'no-install',
    '--headless',
    '--chrome',
    '--chromedriver',
    chromeDriver,
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
      CHROMEDRIVER: chromeDriver,
      WASM_BINDGEN_TEST_WEBDRIVER_JSON: webdriverJson,
      WASM_PACK_CACHE: wasmPackCachePath,
      PLAYWRIGHT_BROWSERS_PATH: playwrightBrowsersPath,
      PATH: [wasmBindgenBinPath(), driverDir, process.env.PATH]
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
