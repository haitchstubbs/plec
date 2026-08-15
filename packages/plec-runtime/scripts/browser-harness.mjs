// packages/plec-runtime/scripts/run-wasm-tests.mjs
import { spawnSync } from 'node:child_process';
import path from 'node:path';

const driverDir = path.resolve(
  import.meta.dirname,
  '../../../.tools/chromedriver-151/chromedriver-win64',
);

const result = spawnSync(
  'wasm-pack',
  ['test', '--headless', '--chrome', 'crates/runtime'],
  {
    stdio: 'inherit',
    env: {
      ...process.env,
      PATH: `${driverDir}${path.delimiter}${process.env.PATH}`,
    },
  },
);

process.exit(result.status ?? 1);
