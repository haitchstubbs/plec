import type { ApplicationIr } from '@wasm-runtime/ir'

export function CompiledApplicationShell({ id, ir }: { id: string; ir: ApplicationIr; pathname: string }) {
  // React owns this host element only. The WASM runtime owns every descendant:
  // rendering a static IR subtree through React causes any later React commit
  // to restore the empty query-loop shell and erase runtime-inserted rows.
  return <div id={id} data-runtime-revision={ir.revision ?? ''} suppressHydrationWarning />
}
