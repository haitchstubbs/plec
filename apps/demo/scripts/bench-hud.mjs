const ANSI = /\x1b\[[0-9;?]*[A-Za-z]/g

export function estimateRemainingMs(elapsedMs, completed, total) {
  if (completed <= 0 || total <= completed) return null
  return Math.round((elapsedMs / completed) * (total - completed))
}

export function formatDuration(ms) {
  if (ms == null || !Number.isFinite(ms)) return 'estimating…'
  if (ms < 1_000) return `${Math.round(ms)}ms`
  const seconds = Math.round(ms / 1_000)
  return seconds < 60 ? `${seconds}s` : `${Math.floor(seconds / 60)}m ${seconds % 60}s`
}

export function formatHud(state, now = performance.now()) {
  const elapsed = Math.max(0, now - state.startedAt)
  const job = state.jobTotal ? `job ${state.jobIndex}/${state.jobTotal}` : 'setup'
  const target = [state.renderer, state.size && `${state.size.toLocaleString()} rows`, state.operation].filter(Boolean).join(' · ')
  const progress = state.sampleTotal ? `${state.stage}: ${state.completed}/${state.sampleTotal}` : state.stage
  const eta = estimateRemainingMs(Math.max(0, now - (state.progressStartedAt ?? state.startedAt)), state.completed, state.sampleTotal)
  return [
    `benchmark  ${state.phase}  |  ${job}${target ? `  |  ${target}` : ''}`,
    `${progress}  |  elapsed ${formatDuration(elapsed)}${eta == null ? '' : `  |  ETA ${formatDuration(eta)}`}`,
  ]
}

export function stripAnsi(value) { return value.replace(ANSI, '') }

export function createBenchmarkHud({ tty = Boolean(process.stdout.isTTY), write = (value) => process.stdout.write(value), intervalMs = 250 } = {}) {
  const state = { phase: 'starting', stage: 'waiting', jobIndex: 0, jobTotal: 0, renderer: null, size: null, operation: null, completed: 0, sampleTotal: 0, startedAt: performance.now() }
  let timer
  let renderedLines = 0
  let suspended = false
  let lastLog = ''

  function clear() {
    if (!tty || renderedLines === 0) return
    write(`\x1b[${renderedLines}A`)
    for (let index = 0; index < renderedLines; index += 1) write('\x1b[2K\r\n')
    write(`\x1b[${renderedLines}A`)
    renderedLines = 0
  }
  function draw(force = false) {
    if (suspended) return
    const lines = formatHud(state)
    if (tty) {
      clear()
      write(`${lines.join('\n')}\n`)
      renderedLines = lines.length
      return
    }
    const line = stripAnsi(lines.join(' | '))
    const milestone = !state.sampleTotal || state.completed === 0 || state.completed === state.sampleTotal || state.completed % Math.max(1, Math.ceil(state.sampleTotal / 10)) === 0
    if ((force || milestone) && line !== lastLog) {
      write(`[bench] ${line}\n`)
      lastLog = line
    }
  }
  function set(next, { force = false } = {}) {
    if ((next.phase === 'warmup' || next.phase === 'sampling') && next.phase !== state.phase) next.progressStartedAt = performance.now()
    Object.assign(state, next)
    draw(force)
  }
  function suspend() { clear(); suspended = true }
  function resume() { suspended = false; draw(true) }
  function start() { timer = setInterval(() => draw(), intervalMs); draw(true) }
  function stop() { if (timer) clearInterval(timer); timer = undefined; clear() }
  return { state, set, start, stop, suspend, resume, draw }
}
