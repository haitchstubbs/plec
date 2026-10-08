import { copyFile, stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { currentNativeTarget } from '../src/native-platform.ts';

const packageRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const target = currentNativeTarget();
const filename = `index.${target}.node`;
const source = path.join(packageRoot, 'native', filename);
const destination = path.join(packageRoot, 'npm', target, filename);

await stat(source);
await copyFile(source, destination);
console.info(`Staged local native binding for ${target}`);
