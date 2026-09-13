import { defineConfig } from '@playwright/test';
import { baseURL, webServer } from './config.shared';

// Opt-in tier: cold navigation benchmark. Sample count comes from
// PLEC_BENCH_SAMPLES (default 10) inside the spec; the @smoke-tagged test
// runs a single sample and is selected by the test:bench:smoke script.
// Tracing and video are off so the harness does not distort what it
// measures.
export default defineConfig({
  testDir: './tests/bench',
  testMatch: '**/*.playwright.@(ts|tsx|js|jsx|mjs|cjs)',
  workers: 1,
  timeout: 15 * 60_000,
  reporter: 'line',
  use: {
    baseURL,
    trace: 'off',
    video: 'off',
  },
  webServer,
});
