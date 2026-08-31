// apps/fullstack/scripts/browser.mjs
import { chromium } from 'playwright';

// Launch order: PLEC_CHROME_EXECUTABLE, then a system Chrome via the
// 'chrome' channel, then Playwright's bundled Chromium — so acceptance
// and benchmark scripts run with no environment setup.
export async function launchBrowser(options = {}) {
  if (process.env.PLEC_CHROME_EXECUTABLE) {
    return chromium.launch({
      ...options,
      executablePath: process.env.PLEC_CHROME_EXECUTABLE,
    });
  }

  try {
    return await chromium.launch({ ...options, channel: 'chrome' });
  } catch {
    return chromium.launch(options);
  }
}
