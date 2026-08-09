import { useEffect, useRef } from 'react'
import { createFileRoute, useNavigate, useRouterState } from '@tanstack/react-router'
import { mountCompiledApplication } from '@wasm-runtime/browser'
import { CompiledApplicationShell } from '../components/CompiledApplicationShell'
import application from 'virtual:wasm-runtime/compiled-signal-garden-ir'

export const Route = createFileRoute('/compiled-signal-garden')({ component: CompiledSignalGardenRoute })

function CompiledSignalGardenRoute() {
  const navigate = useNavigate()
  const pathname = useRouterState({ select: (state) => state.location.pathname })
  const controllerRef = useRef<Awaited<ReturnType<typeof mountCompiledApplication>> | null>(null)
  useEffect(() => {
    const root = document.querySelector<HTMLDivElement>('#compiled-signal-garden-root')
    if (!root) return
    void mountCompiledApplication({ root, irUrl: '/signal-garden.ir.json', hostValues: { currentYear: new Date().getFullYear(), location: { pathname } }, onNavigate: ({ href, replace }) => { void navigate({ to: href as any, replace }) } }).then((mounted) => { controllerRef.current = mounted })
    return () => { controllerRef.current?.dispose(); controllerRef.current = null }
  }, [navigate])
  useEffect(() => { controllerRef.current?.applyHostValues({ location: { pathname } }) }, [pathname])
  return <CompiledApplicationShell id="compiled-signal-garden-root" ir={application} pathname={pathname} />
}
