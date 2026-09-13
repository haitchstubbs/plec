import { defineConfig } from '@playwright/test';
import { baseURL, webServer } from './config.shared';

// Opt-in tier: full behavioral acceptance suites ported from the retired
// manual-spawn scripts. Serial because the suites mutate server state.
export default defineConfig({
  testDir: './tests/acceptance',
  testMatch: '**/*.playwright.@(ts|tsx|js|jsx|mjs|cjs)',
  workers: 1,
  timeout: 30_000,
  use: {
    baseURL,
    trace: 'retain-on-failure',
  },
  webServer,
});
