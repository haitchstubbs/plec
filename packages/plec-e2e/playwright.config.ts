import { defineConfig } from '@playwright/test';
import { baseURL, webServer } from './config.shared';

// Default tier: fast smoke gate. Run with `yarn test:e2e` from the repo
// root; deep behavioral suites live in the acceptance config and the
// navigation benchmark in the bench config.
export default defineConfig({
  testDir: './tests/smoke',
  testMatch: '**/*.playwright.@(ts|tsx|js|jsx|mjs|cjs)',
  fullyParallel: true,
  use: {
    baseURL,
    trace: 'retain-on-failure',
  },
  webServer,
});
