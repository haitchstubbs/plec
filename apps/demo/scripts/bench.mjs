import { spawn, spawnSync } from 'node:child_process'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { chromium } from 'playwright'
import { createBenchmarkHud } from './bench-hud.mjs'

const root = path.resolve(import.meta.dirname, '../../..')
const out = path.join(root, 'benchmarks', 'results')
await loadBenchmarkEnv(path.join(root, '.env.benchmark'))
const sizes = (process.env.BENCH_SIZES ?? '10,100,1000,10000,50000').split(',').map(Number)
const renderers = [...new Set((process.env.BENCH_RENDERERS ?? 'react,compiled').split(',').map((value) => value.trim()).filter(Boolean))]
const warmups = Number(process.env.BENCH_WARMUPS ?? 20)
const samples = Number(process.env.BENCH_SAMPLES ?? 1_000)
const fullValidationEvery = Number(process.env.BENCH_FULL_VALIDATION_EVERY ?? 100)
const port = Number(process.env.BENCH_PORT ?? 4173)
const localBin = path.join(root, '.tools', 'wasm-bindgen', 'bin')
const env = { ...process.env, PATH: `${localBin}${path.delimiter}${process.env.PATH}` }
const hud = createBenchmarkHud()
let server
let browser
let cleanupPromise

if (renderers.length === 0 || renderers.some((renderer) => renderer !== 'react' && renderer !== 'compiled')) {
  throw new Error(`BENCH_RENDERERS must be a comma-separated list of react and/or compiled; received: ${process.env.BENCH_RENDERERS ?? ''}`)
}

async function loadBenchmarkEnv(file) {
  try {
    const source = await readFile(file, 'utf8')
    for (const line of source.split(/\r?\n/)) {
      const match = line.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*?)\s*$/)
      if (!match || process.env[match[1]] !== undefined) continue
      const [, key, rawValue] = match
      process.env[key] = rawValue.replace(/^(['"])(.*)\1$/, '$2')
    }
  } catch (error) {
    if (error?.code !== 'ENOENT') throw error
  }
}
const run = (command, args) => new Promise((resolve, reject) => {
  hud.suspend()
  const child = spawn(command, args, { cwd: root, env, stdio: 'inherit', shell: process.platform === 'win32' })
  child.on('exit', (code) => {
    hud.resume()
    code === 0 ? resolve() : reject(new Error(`${command} ${args.join(' ')} exited ${code}`))
  })
})
const percentile = (values, p) => values[Math.min(values.length - 1, Math.ceil(values.length * p) - 1)]
const stats = (values) => { const sorted = [...values].sort((a,b) => a-b); return { median: percentile(sorted,.5), p95: percentile(sorted,.95), p99: percentile(sorted,.99), samples: values } }
const latencyFields = ['mutationApiMs', 'frameworkPropagationMs', 'reconciliationMs', 'domCommitMs', 'adapterPropagationMs', 'wasmDomMs', 'nextAnimationFrameMs', 'playwrightRoundTripMs']
const timingStats = (samples, field) => {
  const values = samples.map((sample) => sample[field]).filter((value) => typeof value === 'number' && Number.isFinite(value))
  return values.length ? stats(values) : null
}
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
const portOwners = (targetPort) => {
  if (process.platform !== 'win32') return []
  const query = `Get-NetTCPConnection -LocalPort ${targetPort} -State Listen -ErrorAction SilentlyContinue | Select-Object -ExpandProperty OwningProcess`
  const result = spawnSync('powershell.exe', ['-NoProfile', '-Command', query], { cwd: root, encoding: 'utf8' })
  const owners = [...new Set(result.stdout.split(/\s+/).filter(Boolean).map(Number).filter(Number.isInteger))]
  if (result.error || (result.status !== 0 && result.stderr.trim())) throw new Error(`Unable to inspect benchmark port ${targetPort}: ${result.stderr || result.error}`)
  return owners
}
async function stopPort(targetPort) {
  const owners = portOwners(targetPort)
  for (const pid of owners) {
    console.log(`[bench] stopping stale listener on port ${targetPort} (pid ${pid})`)
    const result = spawnSync('taskkill.exe', ['/pid', String(pid), '/T', '/F'], { cwd: root, encoding: 'utf8' })
    if (result.status !== 0) throw new Error(`Unable to stop listener on port ${targetPort} (pid ${pid}): ${result.stderr || result.stdout}`)
  }
  for (let attempt = 0; attempt < 50 && portOwners(targetPort).length; attempt += 1) await pause(100)
  if (portOwners(targetPort).length) throw new Error(`Port ${targetPort} is still listening after cleanup.`)
}
async function waitForPreview(targetPort) {
  const url = `http://127.0.0.1:${targetPort}/`
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try { if ((await fetch(url)).ok) return } catch {}
    await pause(100)
  }
  throw new Error(`Preview server did not become reachable on port ${targetPort}.`)
}

async function stopServer() {
  if (!server?.pid || server.exitCode !== null) return
  if (process.platform === 'win32') {
    const killer = spawn('taskkill.exe', ['/pid', String(server.pid), '/T', '/F'], { cwd: root, stdio: 'ignore', windowsHide: true })
    await Promise.race([
      new Promise((resolve) => killer.once('exit', resolve)),
      pause(5_000),
    ])
    if (killer.exitCode === null) killer.unref()
  } else {
    server.kill('SIGTERM')
    for (let attempt = 0; attempt < 20 && server.exitCode === null; attempt += 1) await pause(50)
    if (server.exitCode === null) server.kill('SIGKILL')
  }
  server.unref()
}

async function cleanup() {
  if (cleanupPromise) return cleanupPromise
  cleanupPromise = (async () => {
    hud.set({ phase: 'cleanup', stage: 'stopping owned processes' }, { force: true })
    try { await browser?.close() } catch {}
    try { await stopServer() } catch {}
    try { await stopPort(port) } catch {}
    hud.stop()
  })()
  return cleanupPromise
}

function handleSignal(signal) {
  process.once(signal, async () => {
    process.exitCode = 1
    await cleanup()
    process.exit()
  })
}
handleSignal('SIGINT')
handleSignal('SIGTERM')

async function main() {
  hud.start()
  hud.set({ phase: 'checking tools', stage: 'wasm-bindgen' }, { force: true })
  if (spawnSync(path.join(localBin, process.platform === 'win32' ? 'wasm-bindgen.exe' : 'wasm-bindgen'), ['--version'], { stdio: 'inherit' }).status !== 0) {
    throw new Error(`Missing project wasm-bindgen. Run: cargo install wasm-bindgen-cli --version 0.2.126 --root ${path.join(root,'.tools','wasm-bindgen')}`)
  }
  hud.set({ phase: 'building WASM', stage: 'runtime package' }, { force: true })
  await run('corepack', ['yarn', 'workspace', '@wasm-runtime/runtime', 'build:wasm'])
  hud.set({ phase: 'building demo', stage: 'Vite production build' }, { force: true })
  await run('corepack', ['yarn', 'build:demo'])
  hud.set({ phase: 'port cleanup', stage: `port ${port}` }, { force: true })
  await stopPort(port)
  const previewCommand = `corepack yarn workspace @wasm-runtime/demo preview --host 127.0.0.1 --port ${port} --strictPort`
  hud.set({ phase: 'preview startup', stage: 'waiting for server' }, { force: true })
  server = process.platform === 'win32'
    ? spawn('cmd.exe', ['/d', '/s', '/c', previewCommand], { cwd: root, env, stdio: 'inherit', shell: false })
    : spawn('corepack', ['yarn', 'workspace', '@wasm-runtime/demo', 'preview', '--host', '127.0.0.1', '--port', String(port), '--strictPort'], { cwd: root, env, stdio: 'inherit', shell: false })
  server.unref()
  await waitForPreview(port)
  hud.set({ phase: 'browser launch', stage: 'starting Playwright' }, { force: true })
  browser = await chromium.launch({ headless: true })
  const results = []
  const jobs = renderers.flatMap((renderer) => sizes.flatMap((size) => ['retitle', 'toggle', 'insert', 'remove', 'move'].map((operation) => ({ renderer, size, operation }))))
  for (const [jobOffset, { renderer, size, operation }] of jobs.entries()) {
    hud.set({ phase: 'page navigation', stage: 'loading benchmark route', jobIndex: jobOffset + 1, jobTotal: jobs.length, renderer, size, operation, completed: 0, sampleTotal: 0 }, { force: true })
    const page = await browser.newPage()
    const route = renderer === 'react' ? '/reactive-todos' : '/compiled-todos'
    await page.goto(`http://127.0.0.1:${port}${route}?bench=1&size=${size}`, { waitUntil: 'networkidle' })
    hud.set({ phase: 'page ready', stage: 'waiting for benchmark API' })
    await page.waitForFunction(() => Boolean(window.__wasmRuntimeBenchmark))
    if (renderer === 'compiled') {
      hud.set({ phase: 'compiled mount', stage: 'waiting for all rows' })
      await page.waitForFunction((rowCount) => window.__wasmRuntimeBenchmark.snapshot().compiledRows === rowCount, size)
      await page.waitForFunction(() => window.__wasmRuntimeBenchmark.integrity?.().ok === true)
    }
    hud.set({ phase: 'validation', stage: 'checking baseline DOM' })
    const before = await page.evaluate(() => window.__wasmRuntimeBenchmark.snapshot())
    await page.evaluate(() => window.__wasmRuntimeBenchmark.measure('retitle'))
    const afterTitle = await page.evaluate(() => window.__wasmRuntimeBenchmark.snapshot())
    if (afterTitle.title === before.title) throw new Error(`${renderer}: retitle correctness check failed`)
    await page.evaluate(() => window.__wasmRuntimeBenchmark.measure('toggle'))
    const afterToggle = await page.evaluate(() => window.__wasmRuntimeBenchmark.snapshot())
    if (afterToggle.done === afterTitle.done) throw new Error(`${renderer}: toggle correctness check failed`)
    await page.evaluate(() => window.__wasmRuntimeBenchmark.measure('insert'))
    const afterInsert = await page.evaluate(() => window.__wasmRuntimeBenchmark.snapshot())
    if (afterInsert.collectionRows !== afterToggle.collectionRows + 1) throw new Error(`${renderer}: insert correctness check failed`)
    await page.evaluate(() => window.__wasmRuntimeBenchmark.prepare('remove'))
    await page.evaluate(() => window.__wasmRuntimeBenchmark.measure('remove'))
    const afterRemove = await page.evaluate(() => window.__wasmRuntimeBenchmark.snapshot())
    if (afterRemove.collectionRows !== afterToggle.collectionRows) throw new Error(`${renderer}: remove correctness check failed`)
    if (renderer === 'compiled' && afterRemove.compiledRows === 0) throw new Error('compiled: initial WASM row rendering check failed')
    if (!await page.evaluate(() => window.__wasmRuntimeBenchmark.validate())) throw new Error(`${renderer}: initial full DOM validation failed`)
    hud.set({ phase: 'warmup', stage: 'warmups', completed: 0, sampleTotal: warmups })
    for (let i=0;i<warmups;i++) {
      await page.evaluate((op) => window.__wasmRuntimeBenchmark.prepare(op), operation)
      await page.evaluate((op) => window.__wasmRuntimeBenchmark.measure(op), operation)
      hud.set({ completed: i + 1 })
    }
    const noopRoundTrips=[]
    for (let i=0;i<samples;i++) { const start=performance.now(); await page.evaluate(() => 0); noopRoundTrips.push(performance.now()-start) }
    const values=[]
    hud.set({ phase: 'sampling', stage: 'samples', completed: 0, sampleTotal: samples })
    for (let i=0;i<samples;i++) {
      await page.evaluate((op) => window.__wasmRuntimeBenchmark.prepare(op), operation)
      const start=performance.now()
      const sample=await page.evaluate((op) => window.__wasmRuntimeBenchmark.measure(op), operation)
      values.push({ ...sample, playwrightRoundTripMs: performance.now()-start })
      if (renderer === 'compiled') {
        const integrity = await page.evaluate(() => window.__wasmRuntimeBenchmark.integrity())
        if (!integrity.ok) throw new Error(`compiled benchmark integrity failure after sample ${i + 1}: ${JSON.stringify(integrity)}`)
      }
      if (fullValidationEvery > 0 && (i + 1) % fullValidationEvery === 0 && !await page.evaluate(() => window.__wasmRuntimeBenchmark.validate())) throw new Error(`${renderer}: full DOM validation failed after sample ${i + 1}`)
      hud.set({ completed: i + 1 })
    }
    const latency=Object.fromEntries(latencyFields.map((field) => [field, timingStats(values, field)]))
    results.push({ renderer, size, operation, synchronousCallMs:stats(values.map((sample) => sample.mutationApiMs)), latency, playwrightNoopRoundTripMs:stats(noopRoundTrips) })
    hud.set({ phase: 'operation complete', stage: `${values.length} samples` }, { force: true })
    await page.close()
  }
  hud.set({ phase: 'report write', stage: 'writing JSON and Markdown' }, { force: true })
  await mkdir(out,{recursive:true}); const stamp=new Date().toISOString().replace(/[:.]/g,'-'); const jsonPath=path.join(out,`benchmark-${stamp}.json`); const reportPath=path.join(out,`benchmark-${stamp}.md`)
  const payload={version:2, generatedAt:new Date().toISOString(), configuration:{renderers,sizes,warmups,samples,fullValidationEvery}, results}
  await writeFile(jsonPath,JSON.stringify(payload,null,2))
  const formatRows = results.flatMap((result) => [
    ['mutationApiMs', result.synchronousCallMs],
    ['playwrightNoopRoundTripMs', result.playwrightNoopRoundTripMs],
    ...Object.entries(result.latency).filter(([, value]) => value).map(([name, value]) => [name, value]),
  ].map(([metric, value]) => `| ${result.renderer} | ${result.size} | ${result.operation} | ${metric} | ${value.median.toFixed(3)} | ${value.p95.toFixed(3)} | ${value.p99.toFixed(3)} |`))
  const lines=['# WASM Runtime Settled-DOM Benchmark','',`Generated: ${payload.generatedAt}`,'',`Warmups: ${warmups}; samples: ${samples}; full validation every: ${fullValidationEvery}`,'','| Renderer | Rows | Operation | Metric | Median ms | p95 ms | p99 ms |','|---|---:|---|---|---:|---:|---:',...formatRows,'','`domCommitMs` is the same operation-local MutationObserver boundary for both renderers. `nextAnimationFrameMs` is the next paint opportunity, not proof of physical presentation. React reconciliation is optional Profiler telemetry. Playwright round-trip includes browser protocol overhead; compare it with `playwrightNoopRoundTripMs`.']
  await writeFile(reportPath,lines.join('\n'))
  hud.suspend()
  console.log(`[bench] JSON: ${jsonPath}\n[bench] report: ${reportPath}`)
  hud.resume()
}

try {
  await main()
} catch (error) {
  hud.suspend()
  console.error(error)
  process.exitCode = 1
} finally {
  await cleanup()
}
