import { buildClient } from '@plec/vite/build';

let input = '';
for await (const chunk of process.stdin) input += chunk;

try {
  const result = await buildClient(JSON.parse(input));
  process.stdout.write(JSON.stringify(result));
} catch (error) {
  process.stderr.write(`${error?.stack ?? error}\n`);
  process.exitCode = 1;
}
