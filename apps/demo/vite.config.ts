import { defineConfig } from 'vite'
import { devtools } from '@tanstack/devtools-vite'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { tanstackStart } from '@tanstack/react-start/plugin/vite'
import { experimentalRuntime } from '../../packages/vite-plugin/src/index.ts'

import viteReact from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

const config = defineConfig({
  resolve: {
    alias: {
      '@wasm-runtime/browser': path.resolve(fileURLToPath(new URL('.', import.meta.url)), '../../packages/browser/src/index.ts'),
      '@wasm-runtime/compiler': path.resolve(fileURLToPath(new URL('.', import.meta.url)), '../../packages/compiler/src/index.ts'),
      '@wasm-runtime/ir': path.resolve(fileURLToPath(new URL('.', import.meta.url)), '../../packages/ir/src/index.ts'),
      '@wasm-runtime/tanstack-adapter': path.resolve(fileURLToPath(new URL('.', import.meta.url)), '../../packages/tanstack-adapter/src/index.ts'),
      '@wasm-runtime/vite-plugin': path.resolve(fileURLToPath(new URL('.', import.meta.url)), '../../packages/vite-plugin/src/index.ts'),
    },
    tsconfigPaths: true,
  },
  plugins: [
    devtools(),
    experimentalRuntime({ emitIrPath: 'dist/application.ir.json', sourceFile: 'src/compiler-fixtures/compiled-todos-route.tsx', mode: 'lenient', virtualModuleId: 'virtual:wasm-runtime/compiled-todos-ir', controllerVirtualModuleId: 'virtual:wasm-runtime/compiled-todos-controller' }),
    experimentalRuntime({ emitIrPath: 'dist/signal-garden.ir.json', sourceFile: 'src/compiler-fixtures/compiled-signal-garden.tsx', mode: 'lenient', virtualModuleId: 'virtual:wasm-runtime/compiled-signal-garden-ir' }),
    tailwindcss(),
    tanstackStart(),
    viteReact(),
  ],
})

export default config
