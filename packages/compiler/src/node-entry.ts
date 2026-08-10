import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import path from "node:path";
import { compile, type CompileOptions } from "./index.js";

export interface SourceGraphModule { id: string; source: string; filePath: string }
export interface CompiledSourceEntry { modules: SourceGraphModule[]; revision: string; result: ReturnType<typeof compile> }

/** Resolve the same local TS/TSX graph for the Vite adapter and non-Vite apps. */
export async function readSourceGraph(entry: string, rootDir: string, repoRootDir: string, seen = new Set<string>(), canonicalId?: string): Promise<SourceGraphModule[]> {
  const absolute = path.resolve(entry);
  if (seen.has(absolute)) return [];
  seen.add(absolute);
  const source = await readFile(absolute, "utf8");
  const imports = [...source.matchAll(/from\s+["']([^"']+)["']/g)].map((match) => match[1]!);
  const currentId = canonicalId ?? path.relative(rootDir, absolute).replace(/\\/g, "/");
  const nested = await Promise.all(imports.map(async (specifier) => {
    const dependency = await resolveWorkspaceModule(specifier, repoRootDir) ?? resolveDependencyModule(specifier, absolute);
    if (dependency) return readSourceGraph(dependency, rootDir, repoRootDir, seen, logicalModuleId(currentId, specifier));
    if (!specifier.startsWith(".")) return [];
    const base = path.resolve(path.dirname(absolute), specifier);
    for (const candidate of [`${base}.tsx`, `${base}.ts`, path.join(base, "index.tsx"), path.join(base, "index.ts")]) {
      try { return await readSourceGraph(candidate, rootDir, repoRootDir, seen, logicalModuleId(currentId, specifier)); } catch (error: any) { if (error?.code !== "ENOENT" && error?.code !== "EISDIR") throw error; }
    }
    return [];
  }));
  return [{ id: currentId, source, filePath: absolute }, ...nested.flat()];
}

export async function compileSourceEntry(entry: string, options: { rootDir: string; repoRootDir: string; mode?: CompileOptions["mode"]; rootComponent?: string }): Promise<CompiledSourceEntry> {
  const modules = await readSourceGraph(entry, options.rootDir, options.repoRootDir);
  const revision = createHash("sha256").update(modules.map((module) => `${module.id}\n${module.source}`).join("\n")).digest("hex");
  const source = modules[0]?.source ?? "";
  return { modules, revision, result: compile(source, { mode: options.mode, rootComponent: options.rootComponent, moduleId: modules[0]?.id, modules, applicationRevision: revision }) };
}

function logicalModuleId(from: string, specifier: string): string {
  if (!specifier.startsWith(".")) return specifier;
  const base = from.split("/"); if (/\.(?:[cm]?[jt]sx?)$/.test(from)) base.pop();
  for (const part of specifier.split("/")) { if (!part || part === ".") continue; if (part === "..") base.pop(); else base.push(part); }
  const resolved = base.join("/"); return /\.(?:[cm]?[jt]sx?)$/.test(resolved) ? resolved : `${resolved}.tsx`;
}
function resolveDependencyModule(specifier: string, fromFile: string): string | null {
  if (specifier.startsWith(".") || specifier.startsWith("node:")) return null;
  try { const resolved = createRequire(fromFile).resolve(specifier); const esm = resolved.replace(/\.js$/, ".mjs"); return existsSync(esm) ? esm : resolved; } catch { return null; }
}
async function resolveWorkspaceModule(specifier: string, repoRootDir: string): Promise<string | null> {
  const match = specifier.match(/^@wasm-runtime\/([^/]+)(\/.*)?$/); if (!match) return null;
  try {
    const packageDir = path.resolve(repoRootDir, "packages", match[1]!);
    const manifest = JSON.parse(await readFile(path.join(packageDir, "package.json"), "utf8")) as { exports?: Record<string, string> };
    const requested = `.${match[2] ?? ""}`;
    for (const [pattern, target] of Object.entries(manifest.exports ?? {})) {
      if (pattern === requested) return path.resolve(packageDir, target);
      const wildcard = pattern.indexOf("*");
      if (wildcard < 0) continue;
      const prefix = pattern.slice(0, wildcard); const suffix = pattern.slice(wildcard + 1);
      if (!requested.startsWith(prefix) || !requested.endsWith(suffix)) continue;
      return path.resolve(packageDir, target.replace("*", requested.slice(prefix.length, requested.length - suffix.length)));
    }
  } catch (error: any) { if (error?.code !== "ENOENT") throw error; }
  return null;
}
