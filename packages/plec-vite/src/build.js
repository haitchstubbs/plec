import path from 'node:path';
import { readFile, rm } from 'node:fs/promises';
import { build as viteBuild } from 'vite';

const VIRTUAL_PREFIX = '\0plec-provider:';
const FORBIDDEN = ['zod', 'typescript', '@swc/core'];

/** Build Plec's browser graph with Vite and return a stable Plec-facing map. */
export async function buildClient({
  root,
  entry,
  outDir,
  providers = {},
  optimize = true,
}) {
  const absoluteRoot = path.resolve(root);
  const providerEntries = Object.fromEntries(
    Object.keys(providers).map((id) => [
      `plec_provider_${inputName(id)}`,
      `plec-provider:${id}`,
    ]),
  );
  const emittedEntries = new Map();
  let manifest;
  await viteBuild({
    root: absoluteRoot,
    base: '/_plec/',
    logLevel: 'warn',
    plugins: [
      {
        name: 'plec-production-client',
        resolveId(source) {
          if (source.startsWith('plec-provider:'))
            return `${VIRTUAL_PREFIX}${source.slice('plec-provider:'.length)}`;
        },
        async load(id) {
          if (!id.startsWith(VIRTUAL_PREFIX)) return;
          const provider = id.slice(VIRTUAL_PREFIX.length);
          const declaration = providers[provider];
          if (!declaration)
            throw new Error(`unknown Plec provider ${provider}`);
          const names = [...declaration.components];
          if (names.some((name) => !/^[$A-Z_a-z][$\w]*$/.test(name))) {
            throw new Error(
              `provider ${provider} contains an invalid component name`,
            );
          }
          const imports = names.join(', ');
          const resolved = await this.resolve(
            declaration.adapter,
            path.join(absoluteRoot, '__plec_provider__.js'),
            { skipSelf: true },
          );
          if (!resolved)
            throw new Error(
              `cannot resolve provider adapter ${declaration.adapter} for ${provider}`,
            );
          return `import createProvider, { ${imports} } from ${JSON.stringify(resolved.id)};\nexport default () => createProvider({ ${imports} });`;
        },
        generateBundle(_options, bundle) {
          for (const output of Object.values(bundle)) {
            if (output.type === 'chunk' && output.isEntry) {
              emittedEntries.set(output.name, output.fileName);
            }
          }
          const violations = new Map();
          for (const output of Object.values(bundle)) {
            if (output.type !== 'chunk') continue;
            for (const id of Object.keys(output.modules)) {
              const normalized = id.replaceAll('\\', '/');
              for (const dependency of FORBIDDEN) {
                if (matchesPackage(normalized, dependency)) {
                  const paths = violations.get(dependency) ?? [];
                  paths.push(normalized);
                  violations.set(dependency, paths);
                }
              }
            }
          }
          if (violations.size) {
            throw new Error(
              `[PLEC-DEPENDENCY-VALIDATION] forbidden dependency leaked into browser bundle:\n${[...violations].map(([name, paths]) => `${name}:\n${paths.map((item) => `  - ${item}`).join('\n')}`).join('\n')}`,
            );
          }
        },
      },
    ],
    build: {
      write: true,
      outDir: path.resolve(outDir),
      emptyOutDir: false,
      manifest: true,
      minify: optimize ? 'esbuild' : false,
      target: 'es2022',
      assetsInlineLimit: 0,
      esbuild: { jsx: 'automatic', jsxImportSource: 'plec' },
      rollupOptions: {
        preserveEntrySignatures: 'strict',
        input: {
          client: path.resolve(absoluteRoot, entry),
          ...providerEntries,
        },
        output: {
          entryFileNames: 'assets/[name]-[hash].js',
          chunkFileNames: 'assets/[name]-[hash].js',
          assetFileNames: 'assets/[name]-[hash][extname]',
        },
      },
    },
  });
  manifest = await readFile(
    path.join(outDir, '.vite/manifest.json'),
    'utf8',
  );
  await rm(path.join(outDir, '.vite'), {
    recursive: true,
    force: true,
  });
  const viteManifest = JSON.parse(manifest);
  const clientManifestKey = Object.keys(viteManifest).find(
    (key) =>
      viteManifest[key].isEntry && viteManifest[key].name === 'client',
  );
  const client = clientManifestKey
    ? viteManifest[clientManifestKey]
    : null;
  const clientFile = emittedEntries.get('client') ?? client?.file;
  if (!clientFile)
    throw new Error(
      'Vite production manifest is missing the client entry',
    );
  return {
    entry: `/_plec/${clientFile}`,
    styles: [...new Set(client?.css ?? [])].map(
      (file) => `/_plec/${file}`,
    ),
    providers: Object.fromEntries(
      Object.keys(providers).map((id) => {
        const name = `plec_provider_${inputName(id)}`;
        const emittedFile = emittedEntries.get(name);
        if (!emittedFile)
          throw new Error(
            `Vite production manifest is missing provider ${id}`,
          );
        return [id, `/_plec/${emittedFile}`];
      }),
    ),
  };
}

function inputName(value) {
  return Buffer.from(value).toString('hex');
}

function matchesPackage(input, packageName) {
  const parts = packageName.split('/');
  const segments = input.split('/');
  return ['node_modules', 'packages'].some((anchor) =>
    segments.some(
      (segment, index) =>
        segment === anchor &&
        parts.every(
          (part, offset) => segments[index + 1 + offset] === part,
        ),
    ),
  );
}
