import type { PlecPerformanceSnapshot } from '../support/helpers';

export const metricNames = [
  'ttfb',
  'htmlReceived',
  'clientBootstrap',
  'irReceived',
  'wasmCompiledInstantiated',
  'runtimeMountStart',
  'runtimeMountEnd',
  'fcp',
  'lcp',
] as const;

export type MetricName = (typeof metricNames)[number];

export interface Sample {
  metrics: Partial<Record<MetricName, number>>;
  missing: Partial<Record<MetricName, string[]>>;
}

export interface Phase {
  id: string;
  url: string;
  throttle: null | {
    download: number;
    upload: number;
    latency: number;
  };
}

export interface AggregatedPhase {
  samples: Sample[];
  metrics: Record<
    MetricName,
    { median: number; p95: number; samples: number } | null
  >;
  missing: Record<MetricName, string[]>;
}

export function percentile(values: number[], fraction: number): number {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[
    Math.min(sorted.length - 1, Math.ceil(sorted.length * fraction) - 1)
  ]!;
}

export function aggregatePhase(samples: Sample[]): AggregatedPhase {
  const metrics = Object.fromEntries(
    metricNames.map((name) => {
      const values = samples
        .map((sample) => sample.metrics[name])
        .filter(
          (value): value is number =>
            typeof value === 'number' && Number.isFinite(value),
        );
      return [
        name,
        values.length
          ? {
              median: percentile(values, 0.5),
              p95: percentile(values, 0.95),
              samples: values.length,
            }
          : null,
      ];
    }),
  );
  const missing = Object.fromEntries(
    metricNames.map((name) => [
      name,
      samples.flatMap((sample) => sample.missing[name] ?? []),
    ]),
  );
  return { samples, metrics, missing } as AggregatedPhase;
}

export function configuredPhases(
  origin: string | undefined,
  localBase: string,
): Phase[] {
  return [
    { id: 'localhost', url: `${localBase}/`, throttle: null },
    {
      id: 'localhost-fast-4g',
      url: `${localBase}/`,
      throttle: {
        download: (9 * 1024 * 1024) / 8,
        upload: (1.5 * 1024 * 1024) / 8,
        latency: 150,
      },
    },
    ...(origin
      ? [{ id: 'deployed-origin', url: origin, throttle: null }]
      : []),
  ];
}

export function metricsFrom(
  snapshot: PlecPerformanceSnapshot,
): Sample['metrics'] {
  return {
    ttfb: snapshot.navigation.ttfb,
    htmlReceived: snapshot.navigation.htmlReceived,
    clientBootstrap: snapshot.durations.clientBootstrap,
    irReceived: snapshot.durations.irReceived,
    wasmCompiledInstantiated:
      snapshot.durations.wasmCompiledInstantiated,
    runtimeMountStart: snapshot.marks['plec:mount-start'],
    runtimeMountEnd: snapshot.marks['plec:mount-end'],
    fcp: snapshot.fcp,
    lcp: snapshot.lcp,
  };
}
