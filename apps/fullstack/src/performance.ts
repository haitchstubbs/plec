export interface PlecPerformanceSnapshot {
  marks: Record<string, number>;
  navigation: { ttfb?: number; htmlReceived?: number };
  fcp?: number;
  lcp?: number;
  durations: {
    clientBootstrap?: number;
    irReceived?: number;
    wasmCompiledInstantiated?: number;
    runtimeMount?: number;
  };
  missing: string[];
}

declare global {
  interface Window {
    __plecPerformance?: { snapshot(): PlecPerformanceSnapshot };
  }
}

const boundaryMarks = [
  'plec:navigation',
  'plec:artifact-fetch-start',
  'plec:artifact-ready',
  'plec:runtime-init-start',
  'plec:runtime-ready',
  'plec:mount-start',
  'plec:mount-end',
  'plec:mount-error',
];

function latestMark(name: string): number | undefined {
  const entries = performance.getEntriesByName(name, 'mark');
  return entries.length ? entries.at(-1)?.startTime : undefined;
}

function difference(
  end: number | undefined,
  start: number | undefined,
): number | undefined {
  return end === undefined || start === undefined
    ? undefined
    : end - start;
}

export function installPlecPerformance(): void {
  performance.mark('plec:navigation');
  let fcp: number | undefined;
  let lcp: number | undefined;
  const observe = (
    type: 'paint' | 'largest-contentful-paint',
    callback: (entry: PerformanceEntry) => void,
  ) => {
    try {
      const observer = new PerformanceObserver((entries) =>
        entries.getEntries().forEach(callback),
      );
      observer.observe({ type, buffered: true });
    } catch {
      // Older browsers may not support buffered paint or LCP entries.
    }
  };
  observe('paint', (entry) => {
    if (entry.name === 'first-contentful-paint') fcp = entry.startTime;
  });
  observe('largest-contentful-paint', (entry) => {
    lcp = entry.startTime;
  });
  window.__plecPerformance = {
    snapshot(): PlecPerformanceSnapshot {
      const marks = Object.fromEntries(
        boundaryMarks.flatMap((name) => {
          const value = latestMark(name);
          return value === undefined ? [] : [[name, value]];
        }),
      );
      const navigation = performance
        .getEntriesByType('navigation')
        .at(-1) as PerformanceNavigationTiming | undefined;
      const snapshot: PlecPerformanceSnapshot = {
        marks,
        navigation: navigation
          ? {
              ttfb: navigation.responseStart,
              htmlReceived: navigation.responseEnd,
            }
          : {},
        fcp,
        lcp,
        durations: {
          clientBootstrap: difference(
            marks['plec:artifact-fetch-start'],
            marks['plec:navigation'],
          ),
          irReceived: difference(
            marks['plec:artifact-ready'],
            marks['plec:artifact-fetch-start'],
          ),
          wasmCompiledInstantiated: difference(
            marks['plec:runtime-ready'],
            marks['plec:runtime-init-start'],
          ),
          runtimeMount: difference(
            marks['plec:mount-end'],
            marks['plec:mount-start'],
          ),
        },
        missing: [],
      };
      for (const [name, value] of Object.entries({
        ttfb: snapshot.navigation.ttfb,
        htmlReceived: snapshot.navigation.htmlReceived,
        clientBootstrap: snapshot.durations.clientBootstrap,
        irReceived: snapshot.durations.irReceived,
        wasmCompiledInstantiated:
          snapshot.durations.wasmCompiledInstantiated,
        runtimeMountStart: marks['plec:mount-start'],
        runtimeMountEnd: marks['plec:mount-end'],
        fcp,
        lcp,
      })) {
        if (value === undefined) snapshot.missing.push(name);
      }
      return snapshot;
    },
  };
}
