export interface O1PerformanceSnapshot {
  marks: Record<string, number>;
  navigation: { ttfb?: number; htmlReceived?: number };
  fcp?: number;
  lcp?: number;
  durations: { clientBootstrap?: number; irReceived?: number; wasmCompiledInstantiated?: number; runtimeMount?: number };
  missing: string[];
}

declare global {
  interface Window { __o1Performance?: { snapshot(): O1PerformanceSnapshot } }
}

const boundaryMarks = ["o1:navigation", "o1:artifact-fetch-start", "o1:artifact-ready", "o1:runtime-init-start", "o1:runtime-ready", "o1:mount-start", "o1:mount-end", "o1:mount-error"];

function latestMark(name: string): number | undefined {
  const entries = performance.getEntriesByName(name, "mark");
  return entries.length ? entries.at(-1)?.startTime : undefined;
}

function difference(end: number | undefined, start: number | undefined): number | undefined {
  return end === undefined || start === undefined ? undefined : end - start;
}

export function installO1Performance(): void {
  performance.mark("o1:navigation");
  let fcp: number | undefined;
  let lcp: number | undefined;
  const observe = (type: "paint" | "largest-contentful-paint", callback: (entry: PerformanceEntry) => void) => {
    try {
      const observer = new PerformanceObserver((entries) => entries.getEntries().forEach(callback));
      observer.observe({ type, buffered: true });
    } catch {
      // Older browsers may not support buffered paint or LCP entries.
    }
  };
  observe("paint", (entry) => { if (entry.name === "first-contentful-paint") fcp = entry.startTime; });
  observe("largest-contentful-paint", (entry) => { lcp = entry.startTime; });
  window.__o1Performance = {
    snapshot(): O1PerformanceSnapshot {
      const marks = Object.fromEntries(boundaryMarks.flatMap((name) => {
        const value = latestMark(name);
        return value === undefined ? [] : [[name, value]];
      }));
      const navigation = performance.getEntriesByType("navigation").at(-1) as PerformanceNavigationTiming | undefined;
      const snapshot: O1PerformanceSnapshot = {
        marks,
        navigation: navigation ? { ttfb: navigation.responseStart, htmlReceived: navigation.responseEnd } : {},
        fcp,
        lcp,
        durations: {
          clientBootstrap: difference(marks["o1:artifact-fetch-start"], marks["o1:navigation"]),
          irReceived: difference(marks["o1:artifact-ready"], marks["o1:artifact-fetch-start"]),
          wasmCompiledInstantiated: difference(marks["o1:runtime-ready"], marks["o1:runtime-init-start"]),
          runtimeMount: difference(marks["o1:mount-end"], marks["o1:mount-start"]),
        },
        missing: [],
      };
      for (const [name, value] of Object.entries({ ttfb: snapshot.navigation.ttfb, htmlReceived: snapshot.navigation.htmlReceived, clientBootstrap: snapshot.durations.clientBootstrap, irReceived: snapshot.durations.irReceived, wasmCompiledInstantiated: snapshot.durations.wasmCompiledInstantiated, runtimeMountStart: marks["o1:mount-start"], runtimeMountEnd: marks["o1:mount-end"], fcp, lcp })) {
        if (value === undefined) snapshot.missing.push(name);
      }
      return snapshot;
    }
  };
}
