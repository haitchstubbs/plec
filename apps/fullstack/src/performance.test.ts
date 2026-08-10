import { afterEach, describe, expect, it, vi } from "vitest";
import { installO1Performance } from "./performance";

afterEach(() => vi.unstubAllGlobals());

describe("O1 navigation performance", () => {
  it("serializes navigation, O1 boundary, and paint timing data", () => {
    const entries = new Map<string, PerformanceEntry[]>([
      ["o1:navigation", [{ startTime: 10 } as PerformanceEntry]],
      ["o1:artifact-fetch-start", [{ startTime: 15 } as PerformanceEntry]],
      ["o1:artifact-ready", [{ startTime: 30 } as PerformanceEntry]],
      ["o1:runtime-init-start", [{ startTime: 35 } as PerformanceEntry]],
      ["o1:runtime-ready", [{ startTime: 45 } as PerformanceEntry]],
      ["o1:mount-start", [{ startTime: 50 } as PerformanceEntry]],
      ["o1:mount-end", [{ startTime: 60 } as PerformanceEntry]],
    ]);
    vi.stubGlobal("window", {});
    vi.stubGlobal("performance", { mark: vi.fn(), getEntriesByName: (name: string) => entries.get(name) ?? [], getEntriesByType: () => [{ responseStart: 4, responseEnd: 8 }] });
    vi.stubGlobal("PerformanceObserver", class { observe() {} });
    installO1Performance();
    const snapshot = window.__o1Performance!.snapshot();
    expect(snapshot.navigation).toEqual({ ttfb: 4, htmlReceived: 8 });
    expect(snapshot.durations).toMatchObject({ clientBootstrap: 5, irReceived: 15, wasmCompiledInstantiated: 10, runtimeMount: 10 });
    expect(snapshot.missing).toEqual(["fcp", "lcp"]);
  });
});
