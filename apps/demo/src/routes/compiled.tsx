import { useEffect, useRef, useState } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { mountCompiledApplication } from '@wasm-runtime/browser'

export const Route = createFileRoute('/compiled')({
  component: CompiledRoute,
})

function CompiledRoute() {
  const rootRef = useRef<HTMLDivElement | null>(null)
  const [status, setStatus] = useState<'idle' | 'mounting' | 'mounted' | 'error'>('idle')
  const [errorMessage, setErrorMessage] = useState<string>('')

  useEffect(() => {
    let cancelled = false

    async function mountRuntime() {
      if (!rootRef.current) {
        return
      }

      setStatus('mounting')
      try {
        await mountCompiledApplication({ root: rootRef.current })
        if (!cancelled) {
          setStatus('mounted')
        }
      } catch (error) {
        if (!cancelled) {
          setStatus('error')
          setErrorMessage(error instanceof Error ? error.message : String(error))
        }
      }
    }

    mountRuntime()

    return () => {
      cancelled = true
    }
  }, [])

  return (
    <main className="page-wrap px-4 py-12">
      <section className="island-shell rounded-2xl p-6 sm:p-8">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <p className="island-kicker mb-2">Compiled Runtime Route</p>
            <h1 className="display-title m-0 text-4xl font-bold text-[var(--sea-ink)] sm:text-5xl">
              WASM-rendered view mount
            </h1>
          </div>
          <p className="m-0 text-sm font-semibold uppercase tracking-[0.18em] text-[var(--sea-ink-soft)]">
            Runtime status: {status}
          </p>
        </div>
        <p className="mt-4 max-w-3xl text-base leading-8 text-[var(--sea-ink-soft)]">
          React provides the frame, and the runtime owns the mounted content below.
        </p>
        {status === 'error' ? (
          <p className="mt-4 m-0 text-sm text-red-700">{errorMessage}</p>
        ) : null}
        <div
          ref={rootRef}
          id="compiled-root"
          className="mt-6 min-h-24 rounded-xl border border-[var(--line)] bg-white/60 p-4"
        />
      </section>
    </main>
  )
}
