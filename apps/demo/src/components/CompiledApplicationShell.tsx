import { useEffect, useState } from 'react'
import { renderStaticApplication, type ApplicationIr } from '@wasm-runtime/ir'

export function CompiledApplicationShell({ id, ir, pathname }: { id: string; ir: ApplicationIr; pathname: string }) {
  const [relinquished, setRelinquished] = useState(false)
  const html = renderStaticApplication(ir, { currentYear: new Date().getFullYear(), location: { pathname } })
  // Render identical markup for SSR and the first client render so hydration
  // never clears the shell. Immediately afterwards React relinquishes the
  // subtree; the WASM runtime is then its sole owner and may add query rows.
  useEffect(() => { setRelinquished(true) }, [])
  return <div id={id} data-runtime-revision={ir.revision ?? ''} suppressHydrationWarning {...(!relinquished ? { dangerouslySetInnerHTML: { __html: html } } : {})} />
}
