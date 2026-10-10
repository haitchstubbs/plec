import { execFileSync, spawnSync } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { brotliCompressSync, gzipSync, constants } from 'node:zlib';

const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const packageDir = path.join(
  root,
  'packages',
  'plec',
  'dist',
  'runtime',
);
const wasmPath = path.join(packageDir, 'runtime_bg.wasm');
const runtimeJs = pathToFileURL(
  path.join(packageDir, 'runtime.js'),
).href;
const iterations = Number(
  process.env.PLEC_DECODE_BENCH_ITERATIONS ?? 128,
);
const sameCheckoutBaseline = {
  raw: 761798,
  gzip: 304393,
  brotliQ11: 241770,
};
const suppliedBaseline = {
  raw: 1101073,
  gzip: 403217,
  brotliQ11: 305946,
};

if (process.argv[2] === '--measure') {
  await measure(process.argv[3]);
} else {
  let runs;
  try {
    execFileSync(
      'yarn',
      [
        'workspace',
        '@plec/core',
        'build:wasm',
        '--features',
        'decode-bench',
      ],
      {
        cwd: root,
        stdio: 'inherit',
      },
    );
    runs = {};
    for (const mode of ['serde', 'direct']) {
      const result = spawnSync(
        process.execPath,
        [
          '--expose-gc',
          fileURLToPath(import.meta.url),
          '--measure',
          mode,
        ],
        { cwd: root, encoding: 'utf8', env: process.env },
      );
      if (result.status !== 0) {
        process.stderr.write(result.stderr);
        throw new Error(
          `decode benchmark ${mode} failed (${result.status})`,
        );
      }
      runs[mode] = JSON.parse(result.stdout.trim().split('\n').at(-1));
    }
  } finally {
    // Never leave the benchmark-only exports staged as the package runtime.
    execFileSync('yarn', ['workspace', '@plec/core', 'build:wasm'], {
      cwd: root,
      stdio: 'inherit',
    });
  }
  const wasm = await readFile(wasmPath);
  const after = {
    raw: wasm.byteLength,
    gzip: gzipSync(wasm, { level: 9 }).byteLength,
    brotliQ11: brotliCompressSync(wasm, {
      params: { [constants.BROTLI_PARAM_QUALITY]: 11 },
    }).byteLength,
    wasmSha256: createHash('sha256').update(wasm).digest('hex'),
  };
  const report = {
    date: '2026-10-10',
    node: process.version,
    rustc: execFileSync('rustc', ['--version'], {
      encoding: 'utf8',
    }).trim(),
    wasmPack: execFileSync('wasm-pack', ['--version'], {
      encoding: 'utf8',
    }).trim(),
    build:
      'release/full + wasm-pack optimize + wasm-opt -Oz + wasm-tools strip',
    decodeBenchmarkBuild:
      'same release profile with decode-bench feature; both direct and legacy Serde paths share one module',
    compression: { gzipLevel: 9, brotliQuality: 11 },
    suppliedBaseline,
    sameCheckoutBaseline: {
      ...sameCheckoutBaseline,
      wasmSha256:
        '70e910ed49acd55e53c3d5068003f2816da5856266588ccc7d3ba3ac3a64c821',
    },
    after,
    sizeReductionPercent: {
      sameCheckoutBaseline: percentReduction(
        sameCheckoutBaseline,
        after,
      ),
      suppliedBaseline: percentReduction(suppliedBaseline, after),
    },
    decodeBenchIterations: iterations,
    workloads: runs,
    note: 'Same-checkout baseline is the optimized package artifact measured before code changes. Supplied baseline build-profile/toolchain metadata does not match the current pinned build and is retained separately for transparency.',
  };
  const reportPath = path.join(
    root,
    'benchmarks',
    'results',
    'serde-wasm-removal-2026-10-10.json',
  );
  await mkdir(path.dirname(reportPath), { recursive: true });
  await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
}

function percentReduction(before, after) {
  return Object.fromEntries(
    Object.keys(before).map((key) => [
      key,
      Number(
        (((before[key] - after[key]) / before[key]) * 100).toFixed(3),
      ),
    ]),
  );
}

async function measure(mode) {
  const {
    initSync,
    benchmark_decode_application,
    benchmark_serde_decode_application,
    benchmark_decode_ssr_snapshot,
    benchmark_serde_decode_ssr_snapshot,
  } = await import(runtimeJs);
  const exports = initSync({
    module: new WebAssembly.Module(readFileSync(wasmPath)),
  });
  const input = fixtures();
  const artifactCall =
    mode === 'serde'
      ? benchmark_serde_decode_application
      : benchmark_decode_application;
  const snapshotCall =
    mode === 'serde'
      ? benchmark_serde_decode_ssr_snapshot
      : benchmark_decode_ssr_snapshot;
  const wasmLinearMemoryAtInstantiationBytes =
    exports.memory?.buffer.byteLength ?? null;
  for (let i = 0; i < 8; i++) {
    artifactCall(input.artifact);
    snapshotCall(input.snapshot, input.manifest, input.artifact);
  }
  const wasmLinearMemoryAfterWarmupBytes =
    exports.memory?.buffer.byteLength ?? null;
  globalThis.gc?.();
  const artifact = sample(
    () => artifactCall(input.artifact),
    iterations,
    exports.memory,
  );
  const snapshot = sample(
    () => snapshotCall(input.snapshot, input.manifest, input.artifact),
    iterations,
    exports.memory,
  );
  const propertyAccess = measurePropertyAccess();
  process.stdout.write(
    JSON.stringify({
      mode,
      wasmLinearMemoryAtInstantiationBytes,
      wasmLinearMemoryAfterWarmupBytes,
      artifact,
      snapshot,
      propertyAccess,
    }) + '\n',
  );
}

function sample(run, count, memory) {
  globalThis.gc?.();
  const before = process.memoryUsage();
  const wasmBefore = memory?.buffer.byteLength ?? null;
  const start = performance.now();
  let checksum = 0;
  for (let i = 0; i < count; i++) checksum += run();
  const elapsedMs = performance.now() - start;
  globalThis.gc?.();
  const after = process.memoryUsage();
  const wasmAfter = memory?.buffer.byteLength ?? null;
  return {
    iterations: count,
    totalMs: elapsedMs,
    microsecondsPerDecode: (elapsedMs * 1000) / count,
    checksum,
    jsHeapDeltaBytes: after.heapUsed - before.heapUsed,
    externalDeltaBytes: after.external - before.external,
    arrayBufferDeltaBytes: after.arrayBuffers - before.arrayBuffers,
    wasmLinearMemoryBeforeBytes: wasmBefore,
    wasmLinearMemoryAfterBytes: wasmAfter,
    wasmLinearMemoryHighWaterDeltaBytes:
      wasmBefore === null || wasmAfter === null
        ? null
        : wasmAfter - wasmBefore,
  };
}

function measurePropertyAccess() {
  const records = Array.from({ length: 128 }, (_, index) => ({
    handle: index,
  }));
  const iterations = 2_000_000;
  const run = (read) => {
    let checksum = 0;
    const start = performance.now();
    for (let i = 0; i < iterations; i++)
      checksum += read(records[i & 127]);
    return { elapsedMs: performance.now() - start, checksum };
  };
  const direct = run((record) => record.handle);
  const reflect = run((record) => Reflect.get(record, 'handle'));
  return {
    iterations,
    directMs: direct.elapsedMs,
    reflectMs: reflect.elapsedMs,
    reflectOverDirect: reflect.elapsedMs / direct.elapsedMs,
    checksumsMatch: direct.checksum === reflect.checksum,
  };
}

function fixtures() {
  const artifact = {
    version: '0.10',
    rootComponent: 0,
    components: [
      {
        id: 'bench',
        rootNode: 0,
        strings: ['div'],
        constants: Array.from({ length: 256 }, (_, id) => ({
          id,
          label: `constant-${id}`,
          nested: [true, null, { value: id }],
        })),
        nodes: [{ op: 'element', tag: 0, parent: null }],
        expressions: [{ instructions: [] }],
        actions: [{ instructions: [{ op: 'return' }] }],
      },
    ],
  };
  const manifest = {
    version: 3,
    revision: 'decode-bench-revision',
    rootGraphId: 'bench',
    routes: [
      {
        id: 'bench',
        path: '',
        graphId: 'bench',
        outletId: 'main',
        loaderAction: 0,
      },
    ],
  };
  const exports = Object.fromEntries(
    Array.from({ length: 128 }, (_, index) => {
      const name = `export-${index}`;
      return [
        name,
        {
          value: { level: [index, 'snapshot-value', { ok: true }] },
          declaration: {
            name,
            sourceOwner: 'server',
            valueIsSerializable: true,
            explicitlyPublic: true,
          },
        },
      ];
    }),
  );
  const snapshot = {
    version: 2,
    revision: manifest.revision,
    routes: [{ routeId: 'bench', params: {}, phase: 'active' }],
    public: { location: '/bench', exports },
    loaders: [
      {
        graphId: 'bench',
        action: 0,
        state: {
          kind: 'rejected',
          failure: {
            kind: 'http',
            message: 'request failed (503)',
            status: 503,
            statusText: 'Service Unavailable',
            body: { retry: false, nested: ['busy', 503] },
            url: '/api/bench',
          },
        },
      },
    ],
    structure: {
      graphs: {
        'root/outlet:main': {
          graphId: 'bench',
          branches: [],
          loops: [],
        },
      },
      nested: {},
    },
  };
  return { artifact, manifest, snapshot };
}
