declare module 'virtual:wasm-runtime/compiled-todos-ir' {
  import type { ApplicationIr } from '@wasm-runtime/ir'
  const application: ApplicationIr
  export default application
}

declare module 'virtual:wasm-runtime/compiled-todos-controller' {
  import type { ComponentType } from 'react'
  export const CompiledInputsController: ComponentType<{ publish(id: string, value: unknown): void; props?: Record<string, unknown> }>
}

declare module 'virtual:wasm-runtime/compiled-signal-garden-ir' {
  import type { ApplicationIr } from '@wasm-runtime/ir'
  const application: ApplicationIr
  export default application
}
