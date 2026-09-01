import { mkdir, readdir, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import {
  expect,
  test,
  type Browser,
  type BrowserContext,
} from '@playwright/test';
import { baseURL } from '../../config.shared';
import {
  aggregatePhase,
  configuredPhases,
  metricNames,
  metricsFrom,
  type AggregatedPhase,
  type Phase,
  type PlecPerformanceSnapshot,
  type Sample,
} from './bench-utils';

const appDir = path.resolve(
  import.meta.dirname,
  '../../../..',
  'apps/fullstack',
);
const resultsDir = path.resolve(
  import.meta.dirname,
  '../../../..',
  'benchmarks/results',
);
const origin = process.env.PLEC_BENCHMARK_ORIGIN;
const sampleEnv = process.env.PLEC_BENCH_SAMPLES;
const sampleCount = sampleEnv === undefined ? 10 : Number(sampleEnv);
if (!Number.isInteger(sampleCount) || sampleCount < 1) {
  throw new Error('PLEC_BENCH_SAMPLES must be a positive integer');
}

interface PhaseReport {
  id: string;
  url: string;
  throttle: Phase['throttle'];
  result: AggregatedPhase;
}

interface BenchPayload {
  version: number;
  generatedAt: string;
  browser: string;
  sampleCount: number;
  cacheDisabled: boolean;
  artifactBytes: number;
  phases: PhaseReport[];
}

async function directoryBytes(directory: string): Promise<number> {
  let total = 0;
  for (const entry of await readdir(directory, {
    withFileTypes: true,
  })) {
    const target = path.join(directory, entry.name);
    total += entry.isDirectory()
      ? await directoryBytes(target)
      : (await stat(target)).size;
  }
  return total;
}

function renderMarkdown(payload: BenchPayload): string {
  const rows = payload.phases.flatMap((phase) =>
    metricNames.map((metric) => {
      const value = phase.result.metrics[metric];
      return `| ${phase.id} | ${metric} | ${value ? value.median.toFixed(2) : 'n/a'} | ${value ? value.p95.toFixed(2) : 'n/a'} | ${value?.samples ?? 0} |`;
    }),
  );
  return [
    '# Plec Cold Navigation Benchmark',
    '',
    `Generated: ${payload.generatedAt}`,
    '',
    `Browser: ${payload.browser}`,
    `Cache: disabled; context: isolated incognito; samples: ${payload.sampleCount}`,
    '',
    '| Phase | Metric | Median ms | p95 ms | Samples |',
    '|---|---|---:|---:|---:',
    ...rows,
    '',
    origin
      ? 'Deployed origin was included.'
      : 'Deployed origin skipped: set PLEC_BENCHMARK_ORIGIN to include it.',
  ].join('\n');
}

async function samplePage(
  browser: Browser,
  phase: Phase,
): Promise<Sample> {
  // One isolated incognito context per sample so cache state cannot leak.
  const context: BrowserContext = await browser.newContext();
  const page = await context.newPage();
  const cdp = await context.newCDPSession(page);
  await cdp.send('Network.enable');
  await cdp.send('Network.clearBrowserCache');
  await cdp.send('Network.setCacheDisabled', { cacheDisabled: true });
  if (phase.throttle) {
    await cdp.send('Network.emulateNetworkConditions', {
      offline: false,
      latency: phase.throttle.latency,
      downloadThroughput: phase.throttle.download,
      uploadThroughput: phase.throttle.upload,
      connectionType: 'cellular4g',
    });
  }
  try {
    await page.goto(phase.url, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(
      () =>
        window.__plecPerformance?.snapshot().marks['plec:mount-end'] !==
        undefined,
    );
    await page.waitForTimeout(1_000);
    const snapshot: PlecPerformanceSnapshot | undefined =
      await page.evaluate(() => window.__plecPerformance?.snapshot());
    expect(
      snapshot,
      'Plec performance API was not installed',
    ).toBeTruthy();
    const metrics = metricsFrom(snapshot!);
    const missing = Object.fromEntries(
      metricNames.map((name) => [
        name,
        metrics[name] === undefined
          ? [
              `missing from page: ${snapshot!.missing.join(', ') || 'not reported'}`,
            ]
          : [],
      ]),
    ) as Sample['missing'];
    return { metrics, missing };
  } finally {
    await context.close();
  }
}

async function runNavigationBenchmark(
  browser: Browser,
  samples: number,
): Promise<void> {
  const phases: Phase[] = configuredPhases(origin, baseURL);
  const samplesByPhase = new Map<string, Sample[]>();
  const browserVersion = browser.version();

  for (let index = 0; index < samples; index += 1) {
    for (const phase of phases) {
      console.log(`[plec-bench] ${phase.id} ${index + 1}/${samples}`);
      const sample = await samplePage(browser, phase);
      const list = samplesByPhase.get(phase.id) ?? [];
      list.push(sample);
      samplesByPhase.set(phase.id, list);
    }
  }

  const phaseReports: PhaseReport[] = phases.map((phase) => ({
    id: phase.id,
    url: phase.url,
    throttle: phase.throttle,
    result: aggregatePhase(samplesByPhase.get(phase.id) ?? []),
  }));
  const payload: BenchPayload = {
    version: 1,
    generatedAt: new Date().toISOString(),
    browser: browserVersion,
    sampleCount: samples,
    cacheDisabled: true,
    artifactBytes: await directoryBytes(
      path.join(appDir, 'dist', 'public'),
    ),
    phases: phaseReports,
  };
  await mkdir(resultsDir, { recursive: true });
  const stamp = new Date().toISOString().replace(/[:.]/g, '-');
  const json = path.join(resultsDir, `plec-navigation-${stamp}.json`);
  const markdown = path.join(resultsDir, `plec-navigation-${stamp}.md`);
  await writeFile(json, JSON.stringify(payload, null, 2));
  await writeFile(markdown, renderMarkdown(payload));
  console.log(
    `[plec-bench] JSON: ${json}\n[plec-bench] report: ${markdown}`,
  );
}

test('collects cold navigation samples', async ({ browser }) => {
  await runNavigationBenchmark(browser, sampleCount);
});

test(
  'collects a single smoke sample',
  { tag: '@smoke' },
  async ({ browser }) => {
    await runNavigationBenchmark(browser, 1);
  },
);
