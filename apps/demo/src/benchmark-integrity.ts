export type AdapterIntegrityDiagnostics = {
  activeRawSubscriptions?: number
  activeLiveListeners?: number
  disposed?: boolean
} | undefined

export type ControllerIntegrityDiagnostics = {
  activeQuerySubscriptions?: number
  activeActionListeners?: number
  activeIslands?: number
  disposed?: boolean
} | undefined

export type MeasurementIntegrity = {
  reactRendersDuringMeasurement: number | null
}

export function resetMeasurementIntegrity(): MeasurementIntegrity {
  return { reactRendersDuringMeasurement: null }
}

export function completeMeasurementIntegrity(
  reactCommitsAtStart: number,
  reactCommitsAtEnd: number,
): MeasurementIntegrity {
  return { reactRendersDuringMeasurement: reactCommitsAtEnd - reactCommitsAtStart }
}

export function evaluateCompiledIntegrity({
  adapter,
  controller,
  reactRendersSinceMount,
  measurement,
  unmanagedMutations,
  compiledRouteReactVisualNodesOutsideIslands = 0,
}: {
  adapter: AdapterIntegrityDiagnostics
  controller: ControllerIntegrityDiagnostics
  reactRendersSinceMount: number | null
  measurement: MeasurementIntegrity
  unmanagedMutations: number
  compiledRouteReactVisualNodesOutsideIslands?: number
}) {
  const violations = [
    adapter?.activeRawSubscriptions !== 1 && 'expected one adapter raw subscription',
    adapter?.activeLiveListeners !== 1 && 'expected one adapter live listener',
    controller?.activeQuerySubscriptions !== 1 && 'expected one compiled query subscription',
    controller?.activeActionListeners !== 1 && 'expected one compiled action listener',
    controller?.activeIslands !== 1 && 'expected one compiled route React island',
    controller?.disposed && 'compiled controller is disposed',
    adapter?.disposed && 'adapter is disposed',
    measurement.reactRendersDuringMeasurement != null
      && measurement.reactRendersDuringMeasurement > 0
      && `React rendered during measured mutation (${measurement.reactRendersDuringMeasurement} commit${measurement.reactRendersDuringMeasurement === 1 ? '' : 's'})`,
    unmanagedMutations > 0 && 'unmanaged TanStack mutations observed',
    compiledRouteReactVisualNodesOutsideIslands > 0 && 'React owns visual nodes outside compiled route islands',
  ].filter(Boolean) as string[]

  return {
    ok: violations.length === 0,
    violations,
    adapter,
    controller,
    reactRendersSinceMount,
    reactRendersDuringMeasurement: measurement.reactRendersDuringMeasurement,
    compiledRouteReactRoots: controller?.activeIslands ?? 0,
    compiledRouteReactVisualNodesOutsideIslands,
    unmanagedMutations,
  }
}
