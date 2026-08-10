export const metricNames = ["ttfb", "htmlReceived", "clientBootstrap", "irReceived", "wasmCompiledInstantiated", "runtimeMountStart", "runtimeMountEnd", "fcp", "lcp"]

export function percentile(values, fraction) {
  const sorted = [...values].sort((a, b) => a - b)
  return sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * fraction) - 1)]
}

export function aggregatePhase(samples) {
  const metrics = Object.fromEntries(metricNames.map((name) => {
    const values = samples.map((sample) => sample.metrics[name]).filter((value) => typeof value === "number" && Number.isFinite(value))
    return [name, values.length ? { median: percentile(values, .5), p95: percentile(values, .95), samples: values.length } : null]
  }))
  const missing = Object.fromEntries(metricNames.map((name) => [name, samples.flatMap((sample) => sample.missing[name] ?? [])]))
  return { samples, metrics, missing }
}

export function configuredPhases(origin) {
  return [
    { id: "localhost", url: "http://127.0.0.1:3199/", throttle: null },
    { id: "localhost-fast-4g", url: "http://127.0.0.1:3199/", throttle: { download: 9 * 1024 * 1024 / 8, upload: 1.5 * 1024 * 1024 / 8, latency: 150 } },
    ...(origin ? [{ id: "deployed-origin", url: origin, throttle: null }] : []),
  ]
}
