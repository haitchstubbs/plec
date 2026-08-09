import { describe, expect, it } from 'vitest'
import { createBenchmarkHud, estimateRemainingMs, formatHud, stripAnsi } from './bench-hud.mjs'

describe('benchmark HUD', () => {
  it('renders phase, target, progress, elapsed time, and ETA', () => {
    const lines = formatHud({ phase: 'sampling', stage: 'samples', jobIndex: 2, jobTotal: 5, renderer: 'compiled', size: 50_000, operation: 'move', completed: 5, sampleTotal: 10, startedAt: 0 }, 1_000)
    expect(lines.join('\n')).toContain('compiled · 50,000 rows · move')
    expect(lines.join('\n')).toContain('samples: 5/10')
    expect(lines.join('\n')).toContain('ETA 1s')
  })

  it('uses plain non-TTY output', () => {
    const output: string[] = []
    const hud = createBenchmarkHud({ tty: false, write: (value: string) => output.push(value) })
    hud.set({ phase: 'mounting', stage: 'compiled mount', renderer: 'compiled', size: 10, operation: 'move' }, { force: true })
    expect(stripAnsi(output.join(''))).toBe(output.join(''))
  })

  it('estimates remaining time from completed samples', () => {
    expect(estimateRemainingMs(1_000, 2, 5)).toBe(1_500)
  })
})
