import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import path from "node:path";
import { compile, type CompileOptions } from "../../compiler/src/index.ts";
import type { Plugin } from "vite";

const runtimeBuilds = new Map<string, Promise<void>>();
const completedRuntimeBuilds = new Set<string>();

export interface ExperimentalRuntimeOptions {
  emitIrPath?: string;
  sourceFile?: string;
  emitRuntimeDir?: string;
  mode?: CompileOptions["mode"];
  /** A Vite virtual module that exposes this exact compiled IR to SSR routes. */
  virtualModuleId?: string;
  /** Optional generated React hook controller for the compiled input boundary. */
  controllerVirtualModuleId?: string;
}

export function experimentalRuntime(options: ExperimentalRuntimeOptions = {}): Plugin {
  const emitIrPath = options.emitIrPath ?? "dist/application.ir.json";
  const emitRuntimeDir = options.emitRuntimeDir ?? "dist/runtime";
  const sourceFile = options.sourceFile ?? "src/routes/index.tsx";
  const mode = options.mode ?? "lenient";
  const virtualModuleId = options.virtualModuleId;
  const resolvedVirtualModuleId = virtualModuleId ? `\0${virtualModuleId}` : undefined;
  const controllerVirtualModuleId = options.controllerVirtualModuleId;
  const resolvedControllerVirtualModuleId = controllerVirtualModuleId ? `\0${controllerVirtualModuleId}.js` : undefined;

  const state = {
    rootDir: process.cwd(),
    repoRootDir: process.cwd(),
    command: "build" as "build" | "serve",
    cachedIr: "",
    cachedController: "",
    controllerVirtualModuleId,
    runtimeArtifacts: [] as Array<{ fileName: string; source: Uint8Array }>,
    sourceFiles: new Set<string>(),
    prepared: false,
  };
  let preparePromise: Promise<void> | null = null;

  return {
    name: "experimental-runtime",
    enforce: "pre",
    resolveId(id) {
      if (id === virtualModuleId) return resolvedVirtualModuleId;
      return id === controllerVirtualModuleId ? resolvedControllerVirtualModuleId : undefined;
    },
    load(id) {
      if (id === resolvedVirtualModuleId) {
        if (!state.cachedIr) throw new Error(`Compiled IR for ${virtualModuleId} is not ready.`);
        return `const application = ${state.cachedIr};\nexport { application };\nexport default application;\n`;
      }
      if (id === resolvedControllerVirtualModuleId) {
        if (!state.cachedController) throw new Error(`Compiled controller for ${controllerVirtualModuleId} is not ready.`);
        return state.cachedController;
      }
      return undefined;
    },
    configResolved(config) {
      state.rootDir = config.root;
      state.repoRootDir = path.resolve(state.rootDir, "..", "..");
      state.command = config.command;
    },
    async configureServer(server) {
      // Vitest loads Vite's server configuration even when it has no browser
      // tests. Do not build WASM/IR as a side effect of a unit-test run.
      if (state.command !== "serve" || server.config.mode === "test" || process.env.VITEST || process.env.NODE_ENV === "test") {
        return;
      }

      preparePromise = prepareArtifacts({
        state,
        rootDir: server.config.root,
        repoRootDir: state.repoRootDir,
        sourceFile,
        mode,
        emitIrPath,
        emitRuntimeDir,
        emitToBundle: false,
      });
      await preparePromise;

      // Public runtime artifacts sit outside Vite's module graph. Rebuild and
      // reload when Rust changes so a long-lived dev server never pairs fresh
      // IR with an older WASM API.
      const runtimeSourceDir = path.resolve(state.repoRootDir, "packages/runtime/crates/runtime/src");
      const runtimeManifest = path.resolve(state.repoRootDir, "packages/runtime/crates/runtime/Cargo.toml");
      server.watcher.add([path.join(runtimeSourceDir, "**/*.rs"), runtimeManifest]);
      server.watcher.on("change", async (file) => {
        const changed = path.resolve(file);
        if (changed !== runtimeManifest && !changed.startsWith(`${runtimeSourceDir}${path.sep}`)) return;
        state.prepared = false;
        completedRuntimeBuilds.delete(state.repoRootDir);
        try {
          preparePromise = prepareArtifacts({ state, rootDir: server.config.root, repoRootDir: state.repoRootDir, sourceFile, mode, emitIrPath, emitRuntimeDir, emitToBundle: false });
          await preparePromise;
          server.ws.send({ type: "full-reload" });
        } catch (error) {
          server.config.logger.error(`Failed to rebuild the experimental runtime: ${error instanceof Error ? error.message : String(error)}`);
        }
      });
    },
    async buildStart() {
      if (state.command !== "build") {
        return;
      }

      await prepareArtifacts({
        state,
        rootDir: state.rootDir,
        repoRootDir: state.repoRootDir,
        sourceFile,
        mode,
        emitIrPath,
        emitRuntimeDir,
        emitToBundle: true,
        pluginContext: this,
      });
    },
    async handleHotUpdate(context) {
      if (state.command !== "serve" || context.server.config.mode === "test" || process.env.VITEST || process.env.NODE_ENV === "test") {
        return;
      }

      // The runtime fetches its IR from `public/`, outside Vite's transformed
      // module graph. Recompile it whenever a local source module changes so
      // a compiled route cannot keep rendering a stale application graph.
      if (!state.sourceFiles.has(path.resolve(context.file))) {
        return;
      }

      state.prepared = false;
      preparePromise = prepareArtifacts({
        state,
        rootDir: context.server.config.root,
        repoRootDir: state.repoRootDir,
        sourceFile,
        mode,
        emitIrPath,
        emitRuntimeDir,
        emitToBundle: false,
      });
      await preparePromise;
      context.server.ws.send({ type: "full-reload" });
      return [];
    },
    async closeBundle() {
      if (!state.cachedIr) {
        return;
      }
      const outputPath = path.resolve(state.rootDir, emitIrPath);
      await mkdir(path.dirname(outputPath), { recursive: true });
      await writeFile(outputPath, state.cachedIr, "utf8");

      const runtimeOutputDir = path.resolve(state.rootDir, emitRuntimeDir);
      await mkdir(runtimeOutputDir, { recursive: true });
      for (const artifact of state.runtimeArtifacts) {
        await writeFile(path.resolve(runtimeOutputDir, artifact.fileName), artifact.source);
      }
    }
  };
}

async function prepareArtifacts(options: {
  state: {
    rootDir: string;
    repoRootDir: string;
    command: "build" | "serve";
    cachedIr: string;
    cachedController: string;
    controllerVirtualModuleId?: string;
    runtimeArtifacts: Array<{ fileName: string; source: Uint8Array }>;
    prepared: boolean;
    sourceFiles: Set<string>;
  };
  rootDir: string;
  repoRootDir: string;
  sourceFile: string;
  mode: CompileOptions["mode"];
  emitIrPath: string;
  emitRuntimeDir: string;
  emitToBundle: boolean;
  pluginContext?: { warn: (message: string) => void; emitFile?: (asset: { type: 'asset'; fileName: string; source: string | Uint8Array }) => void } | null;
}): Promise<void> {
  if (options.state.prepared) {
    return;
  }

  const { state, rootDir, repoRootDir, sourceFile, mode, emitIrPath, emitRuntimeDir, emitToBundle, pluginContext } = options;
  const targetFile = path.resolve(rootDir, sourceFile);
  const modules = await readLocalModuleGraph(targetFile, rootDir, repoRootDir);
  state.sourceFiles = new Set(modules.map((module) => module.filePath));
  const source = modules[0]?.source ?? "";
  const revision = createHash("sha256").update(modules.map((module) => `${module.id}\n${module.source}`).join("\n")).digest("hex");
  const result = compile(source, { mode, moduleId: sourceFile, modules, applicationRevision: revision });
  await buildRuntimeArtifacts(repoRootDir);

  for (const diagnostic of result.diagnostics) {
    pluginContext?.warn(`[${diagnostic.code}] ${diagnostic.message}`);
  }

  state.cachedIr = `${JSON.stringify(result.ir, null, 2)}\n`;
  state.cachedController = state.controllerVirtualModuleId ? generateReactController(source, (result.ir as any).inputs ?? [], targetFile) : "";

  state.runtimeArtifacts = await loadRuntimeArtifacts(repoRootDir);

  if (emitToBundle && pluginContext?.emitFile) {
    pluginContext.emitFile({
      type: "asset",
      fileName: path.basename(emitIrPath),
      source: state.cachedIr
    });

    for (const artifact of state.runtimeArtifacts) {
      pluginContext.emitFile({
        type: "asset",
        fileName: path.posix.join(path.basename(emitRuntimeDir), artifact.fileName),
        source: artifact.source
      });
    }
  }

  await writeDevArtifacts(rootDir, state.cachedIr, state.runtimeArtifacts, path.basename(emitIrPath));
  state.prepared = true;
}

/**
 * The first controller extractor is deliberately mechanical. It keeps only
 * unconditional `const` declarations before the component return and refuses
 * source that needs React control-flow/effect semantics to be sliced.
 */
function generateReactController(source: string, inputs: Array<{ id: string; name: string }>, sourceFile: string): string {
  const bodyMatch = source.match(/function\s+[A-Za-z_$][\w$]*\s*\(([^)]*)\)\s*\{([\s\S]*?)\n\s*return\b/);
  if (!bodyMatch) throw new Error("Compiled input controller requires a function component with a direct return.");
  const parameterSource = bodyMatch[1]!;
  const prefix = bodyMatch[2]!;
  if (/\b(?:if|for|while|switch|try|useEffect|useLayoutEffect|useRef)\b/.test(prefix)) {
    throw new Error("Compiled input controller only supports unconditional pre-return const declarations; use an explicit input producer for this component.");
  }
  const declarations = prefix.split("\n").filter((line) => /^\s*const\s+/.test(line)).join("\n");
  for (const input of inputs) {
    if (!new RegExp(`\\b${escapeRegExp(input.name)}\\b`).test(declarations)) throw new Error(`Compiled input ${input.name} is not declared in the component's unconditional prefix.`);
  }
  const imports = (source.match(/^\s*import[^\n]+;?\s*$/gm) ?? [])
    .filter((line) => !/from\s+["']react["']/.test(line))
    .filter((line) => importIsUsedBy(line, declarations))
    .map((line) => resolveVirtualImport(line, sourceFile))
    .join("\n");
  const destructured = parameterSource.match(/\{([^}]*)\}/)?.[1]?.split(",").map((part) => part.trim().split(/[:=]/)[0]?.trim()).filter(Boolean) ?? [];
  const propsDeclaration = destructured.length ? `const { ${destructured.join(", ")} } = props;` : "";
  const publications = inputs.map((input) => `publish(${JSON.stringify(input.id)}, ${input.name});`).join(" ");
  const dependencies = inputs.map((input) => input.name).join(", ");
  return `import { useEffect } from "react";\n${imports}\nexport function CompiledInputsController({ publish, props = {} }) {\n  ${propsDeclaration}\n${declarations}\n  useEffect(() => { ${publications} }, [${dependencies}]);\n  return null;\n}\n`;
}

function escapeRegExp(value: string): string { return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"); }

function importIsUsedBy(line: string, code: string): boolean {
  const clause = line.match(/^\s*import\s+(?:type\s+)?(.+?)\s+from\s+["']/)?.[1];
  if (!clause) return false;
  const names = [...clause.matchAll(/(?:\{\s*)?([A-Za-z_$][\w$]*)(?:\s+as\s+([A-Za-z_$][\w$]*))?/g)]
    .map((match) => match[2] ?? match[1])
    .filter((name): name is string => Boolean(name && name !== "type"));
  return names.some((name) => new RegExp(`\\b${escapeRegExp(name)}\\b`).test(code));
}

function resolveVirtualImport(line: string, sourceFile: string): string {
  const specifier = line.match(/from\s+(["'])([^"']+)\1/)?.[2];
  if (!specifier?.startsWith(".")) return line;
  const absolute = path.resolve(path.dirname(sourceFile), specifier).replace(/\\/g, "/");
  return line.replace(specifier, `/@fs/${absolute}`);
}

/** The experimental compiler intentionally owns only local component modules. */
async function readLocalModuleGraph(entry: string, rootDir: string, repoRootDir: string, seen = new Set<string>(), canonicalId?: string): Promise<Array<{ id: string; source: string; filePath: string }>> {
  const absolute = path.resolve(entry);
  if (seen.has(absolute)) return [];
  seen.add(absolute);
  const source = await readFile(absolute, "utf8");
  const imports = [...source.matchAll(/from\s+["']([^"']+)["']/g)].map((match) => match[1]!);
  const nested = await Promise.all(imports.map(async (specifier) => {
    const workspaceModule = await resolveWorkspaceModule(specifier, repoRootDir);
    if (workspaceModule) return readLocalModuleGraph(workspaceModule, rootDir, repoRootDir, seen, specifier);
    if (!specifier.startsWith(".")) return [];
    const base = path.resolve(path.dirname(absolute), specifier);
    for (const candidate of [base, `${base}.tsx`, `${base}.ts`, path.join(base, "index.tsx"), path.join(base, "index.ts")]) {
      try { return await readLocalModuleGraph(candidate, rootDir, repoRootDir, seen); } catch (error: any) { if (error?.code !== "ENOENT") throw error; }
    }
    return [];
  }));
  return [{ id: canonicalId ?? path.relative(rootDir, absolute).replace(/\\/g, "/"), source, filePath: absolute }, ...nested.flat()];
}

/** Resolve local workspace package export patterns without treating packages as external. */
async function resolveWorkspaceModule(specifier: string, repoRootDir: string): Promise<string | null> {
  const match = specifier.match(/^@wasm-runtime\/([^/]+)(\/.*)?$/);
  if (!match) return null;
  const packageDir = path.resolve(repoRootDir, "packages", match[1]!);
  try {
    const manifest = JSON.parse(await readFile(path.join(packageDir, "package.json"), "utf8")) as { exports?: Record<string, string> };
    const requested = `.${match[2] ?? ""}`;
    for (const [pattern, target] of Object.entries(manifest.exports ?? {})) {
      if (pattern === requested) return path.resolve(packageDir, target);
      const wildcard = pattern.indexOf("*");
      if (wildcard < 0) continue;
      const prefix = pattern.slice(0, wildcard); const suffix = pattern.slice(wildcard + 1);
      if (!requested.startsWith(prefix) || !requested.endsWith(suffix)) continue;
      const value = requested.slice(prefix.length, requested.length - suffix.length);
      return path.resolve(packageDir, target.replace("*", value));
    }
  } catch (error: any) { if (error?.code !== "ENOENT") throw error; }
  return null;
}

async function buildRuntimeArtifacts(repoRootDir: string): Promise<void> {
  if (completedRuntimeBuilds.has(repoRootDir)) {
    return;
  }

  const activeBuild = runtimeBuilds.get(repoRootDir);
  if (activeBuild) {
    return activeBuild;
  }

  const toolDir = path.resolve(repoRootDir, ".tools", "wasm-bindgen", "bin");
  const executable = path.join(toolDir, process.platform === "win32" ? "wasm-bindgen.exe" : "wasm-bindgen");
  try { await readFile(executable); } catch { throw new Error(`Missing project wasm-bindgen at ${executable}. Install the pinned tool before starting Vite.`); }
  const build = runCommand("corepack", ["yarn", "workspace", "@wasm-runtime/runtime", "build:wasm"], repoRootDir, { PATH: `${toolDir}${path.delimiter}${process.env.PATH ?? ""}` });
  runtimeBuilds.set(repoRootDir, build);
  try {
    await build;
    completedRuntimeBuilds.add(repoRootDir);
  } finally {
    runtimeBuilds.delete(repoRootDir);
  }
}

async function loadRuntimeArtifacts(repoRootDir: string): Promise<Array<{ fileName: string; source: Uint8Array }>> {
  const runtimeDistDir = path.resolve(repoRootDir, "packages/runtime/crates/runtime/dist/runtime");
  const entries = await readdir(runtimeDistDir, { withFileTypes: true });
  const artifacts: Array<{ fileName: string; source: Uint8Array }> = [];

  for (const entry of entries) {
    if (!entry.isFile() || entry.name === "package.json" || entry.name === ".gitignore") {
      continue;
    }

    const fullPath = path.resolve(runtimeDistDir, entry.name);
    const source = await readFile(fullPath);
    artifacts.push({ fileName: entry.name, source });
  }

  if (!artifacts.some((artifact) => artifact.fileName === "runtime.js") || !artifacts.some((artifact) => artifact.fileName.endsWith(".wasm"))) {
    throw new Error(`Runtime build at ${runtimeDistDir} did not produce both runtime.js and a .wasm artifact.`);
  }

  return artifacts;
}

async function writeDevArtifacts(rootDir: string, ir: string, runtimeArtifacts: Array<{ fileName: string; source: Uint8Array }>, irFileName: string): Promise<void> {
  const publicRoot = path.resolve(rootDir, "public");
  await mkdir(path.resolve(publicRoot, "runtime"), { recursive: true });
  await writeFile(path.resolve(publicRoot, irFileName), ir, "utf8");

  for (const artifact of runtimeArtifacts) {
    await writeFile(path.resolve(publicRoot, "runtime", artifact.fileName), artifact.source);
  }
}

async function runCommand(command: string, args: string[], cwd: string, environment?: NodeJS.ProcessEnv): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const isWindows = process.platform === "win32";
    const shellCommand = isWindows ? "powershell" : command;
    const shellArgs = isWindows ? ["-Command", [command, ...args].map(escapeShellArgument).join(" ")] : args;

    const child = spawn(shellCommand, shellArgs, {
      cwd,
      stdio: "inherit", env: environment ? { ...process.env, ...environment } : process.env,
      shell: false
    });

    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) {
        resolve();
        return;
      }
      reject(new Error(`${command} ${args.join(" ")} exited with code ${code ?? "unknown"}`));
    });
  });
}

function escapeShellArgument(value: string): string {
  return /[^A-Za-z0-9_:\/.-]/.test(value) ? `'${value.replace(/'/g, "''")}'` : value;
}

