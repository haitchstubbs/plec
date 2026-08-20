import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { access } from 'node:fs/promises';

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
  const pathEnv = process.env.PATH || process.env.Path || process.env.path;
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
