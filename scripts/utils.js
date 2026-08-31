import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { access } from 'node:fs/promises';
import { existsSync, readdirSync } from 'node:fs';

/**
 * Convert a Unix-style path to Windows path if needed.
 * Git Bash on Windows uses paths like /c/Users/... which need to be converted to C:\Users\...
 *
 * @param {string} unixPath - Unix-style path
 * @returns {string} Windows-style path
 */
function convertToWindowsPath(unixPath) {
  if (!isWindows()) {
    return unixPath;
  }

  // Convert /c/Users/... to C:\Users\...
  const match = unixPath.match(/^\/([a-z])\/(.*)$/);
  if (match) {
    const drive = match[1].toUpperCase();
    const rest = match[2].replace(/\//g, '\\');
    return `${drive}:\\${rest}`;
  }

  return unixPath;
}

/**
 * Check if the current platform is Windows.
 *
 * @returns {boolean}
 */
export function isWindows() {
  return process.platform === 'win32';
}

/**
 * Get the user's home directory cross-platform.
 *
 * @returns {string}
 */
export function getHomeDir() {
  return process.env.HOME || process.env.USERPROFILE || '';
}

/**
 * Find an executable in the system PATH.
 *
 * @param {string} name - Name of the executable to find
 * @returns {Promise<string|null>} Full path to the executable or null if not found
 */
export async function findInPath(name) {
  const pathEnv =
    process.env.PATH || process.env.Path || process.env.path;
  if (!pathEnv) {
    return null;
  }

  const pathSeparator = isWindows() ? ';' : ':';
  const pathDirs = pathEnv.split(pathSeparator);

  for (const dir of pathDirs) {
    const convertedDir = convertToWindowsPath(dir);
    const exePath = path.join(convertedDir, name);
    try {
      // Check if file exists and is executable
      await access(exePath);
      return exePath;
    } catch {
      // Not found in this directory, continue
    }
  }

  return null;
}

/**
 * Locate the Chrome/Chromium binary the browser tooling should drive:
 * PLEC_CHROME_EXECUTABLE if set, otherwise the newest Chromium in the
 * Playwright browser cache. Used to derive the matching ChromeDriver
 * version instead of installing whatever the stable channel ships.
 *
 * @returns {string|undefined} Absolute path to a Chrome/Chromium binary
 */
export function findPlaywrightChromium() {
  if (process.env.PLEC_CHROME_EXECUTABLE) {
    return process.env.PLEC_CHROME_EXECUTABLE;
  }

  const cacheDir =
    process.platform === 'win32'
      ? path.join(process.env.LOCALAPPDATA ?? '', 'ms-playwright')
      : process.platform === 'darwin'
        ? path.join(getHomeDir(), 'Library', 'Caches', 'ms-playwright')
        : path.join(getHomeDir(), '.cache', 'ms-playwright');

  const binaryLayouts = {
    win32: [
      ['chrome-win64', 'chrome.exe'],
      ['chrome-win', 'chrome.exe'],
    ],
    darwin: [
      ['chrome-mac', 'Chromium.app', 'Contents', 'MacOS', 'Chromium'],
    ],
  }[process.platform] ?? [
    ['chrome-linux64', 'chrome'],
    ['chrome-linux', 'chrome'],
  ];

  let revisions;
  try {
    revisions = readdirSync(cacheDir)
      .filter((entry) => /^chromium-\d+$/.test(entry))
      .sort(
        (a, b) =>
          Number(a.slice('chromium-'.length)) -
          Number(b.slice('chromium-'.length)),
      );
  } catch {
    return undefined;
  }

  const newest = revisions.at(-1);
  if (!newest) {
    return undefined;
  }

  for (const segments of binaryLayouts) {
    const binary = path.join(cacheDir, newest, ...segments);
    if (existsSync(binary)) {
      return binary;
    }
  }

  return undefined;
}
