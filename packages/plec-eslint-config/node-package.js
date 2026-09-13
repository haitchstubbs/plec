import { config as baseConfig } from './base.js';

/**
 * Create the ESLint configuration for a Node-oriented workspace package.
 *
 * @param {{ ignores?: string[] }} [options]
 * @returns {import("eslint").Linter.Config[]}
 */
export function nodePackageConfig(options = {}) {
  return [
    ...baseConfig,
    {
      ignores: [
        'native/**',
        'src/generated/**',
        ...(options.ignores ?? []),
      ],
    },
    {
      files: ['scripts/*.mjs'],
      languageOptions: {
        globals: {
          console: 'readonly',
          process: 'readonly',
        },
      },
    },
  ];
}
