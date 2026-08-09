import { describe, expect, it } from 'vitest'
import {
  completeMeasurementIntegrity,
  evaluateCompiledIntegrity,
  resetMeasurementIntegrity,
} from './benchmark-integrity'

const healthyAdapter = { activeRawSubscriptions: 1, activeLiveListeners: 1, disposed: false }
const healthyController = { activeQuerySubscriptions: 1, activeActionListeners: 1, activeIslands: 1, disposed: false }

function integrity(overrides: Partial<Parameters<typeof evaluateCompiledIntegrity>[0]> = {}) {
  return evaluateCompiledIntegrity({
    adapter: healthyAdapter,
    controller: healthyController,
    reactRendersSinceMount: 0,
    measurement: resetMeasurementIntegrity(),
    unmanagedMutations: 0,
    ...overrides,
  })
}

describe('compiled benchmark integrity', () => {
  it('keeps late mount baselines and mount-time renders diagnostic only', () => {
    expect(integrity({ reactRendersSinceMount: null })).toMatchObject({ ok: true, reactRendersSinceMount: null })
    expect(integrity({ reactRendersSinceMount: 3 })).toMatchObject({ ok: true, reactRendersSinceMount: 3 })
  })

  it('reports zero React renders for a normal compiled measurement', () => {
    const measurement = completeMeasurementIntegrity(12, 12)
    expect(integrity({ measurement })).toMatchObject({ ok: true, reactRendersDuringMeasurement: 0 })
  })

  it('fails a measurement that causes React to render', () => {
    const result = integrity({ measurement: completeMeasurementIntegrity(12, 13) })
    expect(result).toMatchObject({ ok: false, reactRendersDuringMeasurement: 1 })
    expect(result.violations).toContain('React rendered during measured mutation (1 commit)')
  })

  it('clears a prior measurement failure before the next warmup or sample', () => {
    const failedMeasurement = completeMeasurementIntegrity(1, 2)
    expect(integrity({ measurement: failedMeasurement }).ok).toBe(false)
    expect(integrity({ measurement: resetMeasurementIntegrity() })).toMatchObject({ ok: true, reactRendersDuringMeasurement: null })
  })

  it('retains subscription, disposal, and unmanaged-mutation violations', () => {
    const result = integrity({
      adapter: { activeRawSubscriptions: 0, activeLiveListeners: 1, disposed: true },
      controller: { activeQuerySubscriptions: 1, activeActionListeners: 0, disposed: true },
      unmanagedMutations: 1,
    })
    expect(result.ok).toBe(false)
    expect(result.violations).toEqual(expect.arrayContaining([
      'expected one adapter raw subscription',
      'expected one compiled action listener',
      'adapter is disposed',
      'compiled controller is disposed',
      'unmanaged TanStack mutations observed',
    ]))
  })
})
