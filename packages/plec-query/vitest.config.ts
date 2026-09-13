import type { ViteUserConfig as UserConfig } from 'vitest/config';

export interface NodeVitestProjectOptions {
  name: string;
  root: string;
  include?: string[];
  exclude?: string[];
  passWithNoTests?: boolean;
  alias?: Record<string, string>;
}

/**
 * Define the shared Node.js Vitest project policy for a workspace package.
 */
export function defineNodeVitestProject({
  name,
  root,
  include = ['tests/**/*.test.ts'],
  exclude = ['tests/types/**'],
  passWithNoTests = false,
  alias,
}: NodeVitestProjectOptions): UserConfig {
  return {
    root,
    resolve: alias === undefined ? undefined : { alias },
    test: {
      name,
      environment: 'node',
      include,
      exclude,
      passWithNoTests,
    },
  };
}

export default defineNodeVitestProject({
  name: 'query',
  root: import.meta.dirname,
});
