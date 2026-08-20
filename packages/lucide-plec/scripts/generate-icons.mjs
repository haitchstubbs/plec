import { mkdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as lucide from 'lucide';

const icons = Object.entries(lucide)
  .filter(([, value]) => Array.isArray(value))
  .filter(([name]) => /^[A-Z][A-Za-z0-9]*$/.test(name))
  .sort(([left], [right]) => left.localeCompare(right));

const packageDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);

const sourceDir = path.join(packageDir, 'src/icons');

const filenameFor = (name) =>
  name.replace(/([a-z0-9])([A-Z])/g, '$1-$2').toLowerCase();

const svgDefaults = {
  xmlns: 'http://www.w3.org/2000/svg',
  width: 24,
  height: 24,
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  'stroke-width': '2',
  'stroke-linecap': 'round',
  'stroke-linejoin': 'round',
};

const jsxValue = (value) => {
  if (typeof value === 'number') {
    return `{${value}}`;
  }

  return JSON.stringify(String(value));
};

const jsxAttributes = (attrs) =>
  Object.entries(attrs)
    .map(([key, value]) => `${key}=${jsxValue(value)}`)
    .join(' ');

await rm(sourceDir, { recursive: true, force: true });

await rm(path.join(packageDir, 'src/icons.generated.ts'), {
  force: true,
});

await mkdir(sourceDir, {
  recursive: true,
});

for (const [name, iconData] of icons) {
  const filename = filenameFor(name);

  const jsxElements = iconData
    .map(([tag, attrs]) => {
      const attrStr = jsxAttributes(attrs);

      return attrStr ? `    <${tag} ${attrStr} />` : `    <${tag} />`;
    })
    .join('\n');

  await writeFile(
    path.join(sourceDir, `${filename}.tsx`),
    [
      '// Generated from the installed lucide package. Do not edit by hand.',
      '// Import standard non-react SVG props from ts',
      'import {} from "plec/jsx-runtime";',
      '',
      `export const ${name} = (props: Record<string, unknown>) => (`,
      '  <svg',
      ...Object.entries(svgDefaults).map(
        ([key, value]) => `    ${key}=${jsxValue(value)}`,
      ),
      '    {...props}',
      '  >',
      jsxElements,
      '  </svg>',
      ');',
      '',
    ].join('\n'),
  );
}
