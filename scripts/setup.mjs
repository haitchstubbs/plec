import { spawnSync } from 'node:child_process';
import cliToolsConfig from '../cli-tools.json' with { type: 'json' };

const { 'build-tools': buildTools } = cliToolsConfig;

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
    process.exit(1);
  }

  if (install.status !== 0) {
    process.exit(install.status ?? 1);
  }

  console.log(`${name} ${version} installed.`);
}

// Ensure required build tools are installed

for (const [name, version] of Object.entries(buildTools)) {
  ensureTool({ name, version });
}

console.log('Ensuring wasm32-unknown-unknown target is installed...');

const target = run('rustup', [
  'target',
  'add',
  'wasm32-unknown-unknown',
]);

if (target.error || target.status !== 0) {
  console.error('Failed to ensure wasm32-unknown-unknown target.');
  process.exit(target.status ?? 1);
}

console.log('WASM toolchain ready.');
