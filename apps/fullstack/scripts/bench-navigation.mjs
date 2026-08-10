import { spawn } from "node:child_process"
import { mkdir, readdir, stat, writeFile } from "node:fs/promises"
import path from "node:path"
import { chromium } from "playwright"
import { aggregatePhase, configuredPhases, metricNames } from "./bench-utils.mjs"

const appDir = path.resolve(import.meta.dirname, "..")
const repoDir = path.resolve(appDir, "..", "..")
const resultsDir = path.join(repoDir, "benchmarks", "results")
const sampleCount = Number(process.env.O1_BENCH_SAMPLES ?? 10)
const port = 3199
const origin = process.env.O1_BENCHMARK_ORIGIN
const headless = process.env.O1_BENCH_HEADLESS === "1"
const executablePath = process.env.O1_CHROME_EXECUTABLE
const localBase = `http://127.0.0.1:${port}`
let server

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
const run = (command, args, options = {}) => new Promise((resolve, reject) => {
  const child = spawn(command, args, { cwd: repoDir, stdio: "inherit", shell: process.platform === "win32", ...options })
  child.once("error", reject)
  child.once("exit", (code) => code === 0 ? resolve() : reject(new Error(`${command} exited ${code}`)))
})

async function waitForLocalServer() {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try { if ((await fetch(`${localBase}/`)).ok) return } catch {}
    await sleep(100)
  }
  throw new Error("Fullstack benchmark server did not become ready")
}

async function directoryBytes(directory) {
  let total = 0
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const target = path.join(directory, entry.name)
    total += entry.isDirectory() ? await directoryBytes(target) : (await stat(target)).size
  }
  return total
}

function metricsFrom(snapshot) {
  return {
    ttfb: snapshot.navigation.ttfb,
    htmlReceived: snapshot.navigation.htmlReceived,
    clientBootstrap: snapshot.durations.clientBootstrap,
    irReceived: snapshot.durations.irReceived,
    wasmCompiledInstantiated: snapshot.durations.wasmCompiledInstantiated,
    runtimeMountStart: snapshot.marks["o1:mount-start"],
    runtimeMountEnd: snapshot.marks["o1:mount-end"],
    fcp: snapshot.fcp,
    lcp: snapshot.lcp,
  }
}

async function sample(browser, phase) {
  const context = await browser.newContext()
  const page = await context.newPage()
  const cdp = await context.newCDPSession(page)
  await cdp.send("Network.enable")
  await cdp.send("Network.clearBrowserCache")
  await cdp.send("Network.setCacheDisabled", { cacheDisabled: true })
  if (phase.throttle) await cdp.send("Network.emulateNetworkConditions", { offline: false, latency: phase.throttle.latency, downloadThroughput: phase.throttle.download, uploadThroughput: phase.throttle.upload, connectionType: "cellular4g" })
  try {
    await page.goto(phase.url, { waitUntil: "domcontentloaded" })
    await page.waitForFunction(() => window.__o1Performance?.snapshot().marks["o1:mount-end"] !== undefined)
    await page.waitForTimeout(1_000)
    const snapshot = await page.evaluate(() => window.__o1Performance?.snapshot())
    if (!snapshot) throw new Error("O1 performance API was not installed")
    const metrics = metricsFrom(snapshot)
    const missing = Object.fromEntries(metricNames.map((name) => [name, metrics[name] === undefined ? [`missing from page: ${snapshot.missing.join(", ") || "not reported"}`] : []]))
    return { metrics, missing, snapshot }
  } finally {
    await context.close()
  }
}

function renderMarkdown(payload) {
  const rows = payload.phases.flatMap((phase) => metricNames.map((metric) => {
    const value = phase.result.metrics[metric]
    return `| ${phase.id} | ${metric} | ${value ? value.median.toFixed(2) : "n/a"} | ${value ? value.p95.toFixed(2) : "n/a"} | ${value?.samples ?? 0} |`
  }))
  return ["# O1 Cold Navigation Benchmark", "", `Generated: ${payload.generatedAt}`, "", `Browser: ${payload.browser}`, `Cache: disabled; context: isolated incognito; samples: ${payload.sampleCount}`, "", "| Phase | Metric | Median ms | p95 ms | Samples |", "|---|---|---:|---:|---:", ...rows, "", origin ? "Deployed origin was included." : "Deployed origin skipped: set O1_BENCHMARK_ORIGIN to include it."].join("\n")
}

async function main() {
  if (!Number.isInteger(sampleCount) || sampleCount < 1) throw new Error("O1_BENCH_SAMPLES must be a positive integer")
  await run("yarn", ["workspace", "@wasm-runtime/fullstack", "build"])
  server = spawn(process.execPath, ["dist/server.mjs"], { cwd: appDir, env: { ...process.env, PORT: String(port) }, stdio: "inherit", windowsHide: true })
  await waitForLocalServer()
  const browser = await chromium.launch({ headless, channel: executablePath ? undefined : "chrome", executablePath })
  try {
    const browserVersion = browser.version()
    const phases = []
    for (const phase of configuredPhases(origin)) {
      const resolved = phase.id === "deployed-origin" ? phase : { ...phase, url: `${localBase}/` }
      const samples = []
      for (let index = 0; index < sampleCount; index += 1) {
        process.stdout.write(`[o1-bench] ${resolved.id} ${index + 1}/${sampleCount}\n`)
        samples.push(await sample(browser, resolved))
      }
      phases.push({ ...resolved, result: aggregatePhase(samples) })
    }
    const payload = { version: 1, generatedAt: new Date().toISOString(), browser: browserVersion, sampleCount, cacheDisabled: true, artifactBytes: await directoryBytes(path.join(appDir, "dist", "public")), phases }
    await mkdir(resultsDir, { recursive: true })
    const stamp = new Date().toISOString().replace(/[:.]/g, "-")
    const json = path.join(resultsDir, `o1-navigation-${stamp}.json`)
    const markdown = path.join(resultsDir, `o1-navigation-${stamp}.md`)
    await writeFile(json, JSON.stringify(payload, null, 2))
    await writeFile(markdown, renderMarkdown(payload))
    console.log(`[o1-bench] JSON: ${json}\n[o1-bench] report: ${markdown}`)
  } finally {
    await browser.close()
  }
}

try { await main() } finally { if (server && server.exitCode === null) server.kill() }
