import { afterEach, describe, expect, it, vi } from 'vitest';
import { installPlecPerformance } from './performance';

afterEach(() => vi.unstubAllGlobals());

describe('Plec navigation performance', () => {
  it('serializes navigation, runtime boundary, and paint timing data', () => {
    const entries = new Map<string, PerformanceEntry[]>([
      ['plec:navigation', [{ startTime: 10 } as PerformanceEntry]],
      [
        'plec:artifact-fetch-start',
        [{ startTime: 15 } as PerformanceEntry],
      ],
      ['plec:artifact-ready', [{ startTime: 30 } as PerformanceEntry]],
      [
        'plec:runtime-init-start',
        [{ startTime: 35 } as PerformanceEntry],
      ],
      ['plec:runtime-ready', [{ startTime: 45 } as PerformanceEntry]],
      ['plec:mount-start', [{ startTime: 50 } as PerformanceEntry]],
      ['plec:mount-end', [{ startTime: 60 } as PerformanceEntry]],
    ]);
    vi.stubGlobal('window', {});
    vi.stubGlobal('performance', {
      mark: vi.fn(),
      getEntriesByName: (name: string) => entries.get(name) ?? [],
      getEntriesByType: () => [{ responseStart: 4, responseEnd: 8 }],
    });
    vi.stubGlobal(
      'PerformanceObserver',
      class {
        observe() {}
      },
    );
    installPlecPerformance();
    const snapshot = window.__plecPerformance!.snapshot();
    expect(snapshot.navigation).toEqual({ ttfb: 4, htmlReceived: 8 });
    expect(snapshot.durations).toMatchObject({
      clientBootstrap: 5,
      irReceived: 15,
      wasmCompiledInstantiated: 10,
      runtimeMount: 10,
    });
    expect(snapshot.missing).toEqual(['fcp', 'lcp']);
  });
});
