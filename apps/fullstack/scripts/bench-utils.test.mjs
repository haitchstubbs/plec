import { describe, expect, it } from "vitest"
import { aggregatePhase, configuredPhases } from "./bench-utils.mjs"

describe("navigation benchmark aggregation", () => {
  it("aggregates valid timings and preserves unavailable metric reasons", () => {
    const report = aggregatePhase([{ metrics: { ttfb: 10, lcp: undefined }, missing: { lcp: ["no LCP entry"] } }, { metrics: { ttfb: 20, lcp: 30 }, missing: {} }])
    expect(report.metrics.ttfb).toMatchObject({ median: 10, p95: 20, samples: 2 })
    expect(report.metrics.lcp).toMatchObject({ median: 30, p95: 30, samples: 1 })
    expect(report.missing.lcp).toEqual(["no LCP entry"])
  })

  it("gates deployed-origin measurements on configuration", () => {
    expect(configuredPhases(undefined)).toHaveLength(2)
    expect(configuredPhases("https://example.test/").at(-1)).toMatchObject({ id: "deployed-origin" })
  })
})
