import { createHash } from 'node:crypto';
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import path from 'node:path';
import { parseSync } from '@swc/core';
import {
  compile,
  compileComponentGraph,
  type CompileOptions,
} from './index.js';

export interface SourceGraphModule {
  id: string;
  source: string;
  filePath: string;
}
export interface CompiledSourceEntry {
  modules: SourceGraphModule[];
  revision: string;
  result: ReturnType<typeof compile>;
}
export interface CompiledRouteGraph {
  path: string;
  graph: ReturnType<typeof compileComponentGraph>['graph'];
  pendingGraph?: ReturnType<typeof compileComponentGraph>['graph'];
  errorGraph?: ReturnType<typeof compileComponentGraph>['graph'];
  /** Numeric handle in graph.actions. Manifests never contain executable code. */
  loaderAction?: number;
  outletId: string;
}
export interface CompiledRouteEntry {
  modules: SourceGraphModule[];
  revision: string;
  rootGraph: ReturnType<typeof compileComponentGraph>['graph'];
  routes: CompiledRouteGraph[];
}

/** Resolve the same local TS/TSX graph for the Vite adapter and non-Vite apps. */
export async function readSourceGraph(
  entry: string,
  rootDir: string,
  repoRootDir: string,
  seen = new Set<string>(),
  canonicalId?: string,
): Promise<SourceGraphModule[]> {
  const absolute = path.resolve(entry);
  if (seen.has(absolute)) return [];
  seen.add(absolute);
  const source = await readFile(absolute, 'utf8');
  const imports = [...source.matchAll(/from\s+["']([^"']+)["']/g)].map(
    (match) => match[1]!,
  );
  const currentId =
    canonicalId ?? path.relative(rootDir, absolute).replace(/\\/g, '/');
  const nested = await Promise.all(
    imports.map(async (specifier) => {
      const dependency =
        (await resolveWorkspaceModule(specifier, repoRootDir)) ??
        resolveDependencyModule(specifier, absolute);
      if (dependency)
        return readSourceGraph(
          dependency,
          rootDir,
          repoRootDir,
          seen,
          logicalModuleId(currentId, specifier),
        );
      if (!specifier.startsWith('.')) return [];
      const base = path.resolve(path.dirname(absolute), specifier);
      // Source packages frequently preserve their published .js/.mjs
      // specifiers in TypeScript source (generated Lucide modules are one
      // example). Resolve the authored source before falling back to a
      // published file so recursive compilation sees the complete graph.
      for (const candidate of sourceCandidates(base)) {
        try {
          return await readSourceGraph(
            candidate,
            rootDir,
            repoRootDir,
            seen,
            logicalModuleId(currentId, specifier),
          );
        } catch (error: any) {
          if (error?.code !== 'ENOENT' && error?.code !== 'EISDIR')
            throw error;
        }
      }
      return [];
    }),
  );
  return [
    { id: currentId, source, filePath: absolute },
    ...nested.flat(),
  ];
}

function sourceCandidates(base: string): string[] {
  const extension = path.extname(base);
  const withoutExtension = extension ? base.slice(0, -extension.length) : base;
  const stems = extension ? [base, withoutExtension] : [base];
  return stems.flatMap((stem) => [
    `${stem}.tsx`,
    `${stem}.ts`,
    `${stem}.jsx`,
    `${stem}.mjs`,
    `${stem}.js`,
    path.join(stem, 'index.tsx'),
    path.join(stem, 'index.ts'),
    path.join(stem, 'index.jsx'),
    path.join(stem, 'index.mjs'),
    path.join(stem, 'index.js'),
  ]);
}

export async function compileSourceEntry(
  entry: string,
  options: {
    rootDir: string;
    repoRootDir: string;
    mode?: CompileOptions['mode'];
    rootComponent?: string;
  },
): Promise<CompiledSourceEntry> {
  const modules = await readSourceGraph(
    entry,
    options.rootDir,
    options.repoRootDir,
  );
  const revision = createHash('sha256')
    .update(
      modules
        .map((module) => `${module.id}\n${module.source}`)
        .join('\n'),
    )
    .digest('hex');
  const source = modules[0]?.source ?? '';
  return {
    modules,
    revision,
    result: compile(source, {
      mode: options.mode,
      rootComponent: options.rootComponent,
      moduleId: modules[0]?.id,
      modules,
      applicationRevision: revision,
    }),
  };
}

export async function compileComponentGraphEntry(
  entry: string,
  options: {
    rootDir: string;
    repoRootDir: string;
    mode?: CompileOptions['mode'];
    rootComponent: string;
  },
) {
  const sourceEntry = await compileSourceEntry(entry, options);
  const source = sourceEntry.modules[0]?.source ?? '';
  return {
    ...sourceEntry,
    result: compileComponentGraph(source, {
      mode: options.mode,
      rootComponent: options.rootComponent,
      moduleId: sourceEntry.modules[0]?.id,
      modules: sourceEntry.modules,
      applicationRevision: sourceEntry.revision,
    }),
  };
}

/** Compile the existing Plec route declarations as metadata, then compile the
 * component modules they reference. Route declaration files deliberately are
 * not component roots themselves. */
export async function compileRouteEntry(
  entry: string,
  options: { rootDir: string; repoRootDir: string; mode?: CompileOptions['mode'] },
): Promise<CompiledRouteEntry> {
  const modules = await readSourceGraph(entry, options.rootDir, options.repoRootDir);
  const revision = createHash('sha256')
    .update(modules.map((module) => `${module.id}\n${module.source}`).join('\n'))
    .digest('hex');
  const byId = new Map(modules.map((module) => [module.id, module]));
  const router = modules[0];
  if (!router) throw new Error('Route entry has no source module.');
  const routerImports = importBindings(router!);
  const routeNames = routeTreeBindings(router!.source);
  if (!routeNames.length)
    throw new Error('Route entry does not contain rootRoute.addChildren([...]).');
  const definitions = routeNames.map((name) => {
    const moduleId = routerImports.get(name);
    const module = moduleId && byId.get(moduleId);
    if (!module) throw new Error(`Route binding ${name} has no available source module.`);
    return parseRouteDefinition(module!, byId);
  });
  const root = definitions.shift();
  if (!root) throw new Error('Route entry does not declare a root route.');
  const compileGraph = (component: RouteComponent) => {
    const source = byId.get(component.moduleId);
    if (!source) throw new Error(`Component source is unavailable: ${component.moduleId}#${component.name}.`);
    return compileComponentGraph(source.source, {
      mode: options.mode ?? 'strict',
      rootComponent: component.name,
      moduleId: source.id,
      modules,
      applicationRevision: revision,
    }).graph;
  };
  return {
    modules,
    revision,
    rootGraph: compileGraph(root!.component),
    routes: definitions.map((definition) => {
      const graph = compileGraph(definition.component);
      let loaderAction: number | undefined;
      if (definition.loader) {
        const state = findLoaderStateSlot(definition.component, graph, byId);
        if (state === undefined)
          throw new Error(`Route loader in ${definition.component.moduleId} has no typed loader state.`);
        loaderAction = graph.actions.length;
        graph.actions.push({
          ...compileRouteLoader(definition.loader, modules, options, revision),
          routeLoader: true,
          loaderResultState: state,
        });
      }
      return {
      path: definition.path,
      graph,
      ...(definition.pendingComponent
        ? { pendingGraph: compileGraph(definition.pendingComponent) }
        : {}),
      ...(definition.errorComponent
        ? { errorGraph: compileGraph(definition.errorComponent) }
        : {}),
      ...(loaderAction === undefined ? {} : { loaderAction }),
      outletId: 'main',
    };
    }),
  };
}

function findLoaderStateSlot(
  component: RouteComponent,
  graph: ReturnType<typeof compileComponentGraph>['graph'],
  modules: Map<string, SourceGraphModule>,
): number | undefined {
  const source = modules.get(component.moduleId)?.source;
  if (!source) return undefined;
  const ast: any = parseSync(source, { syntax: 'typescript', tsx: true, target: 'es2022' });
  let loaderName: string | undefined;
  let stateName: string | undefined;
  const stateNames: string[] = [];
  const visit = (value: any): void => {
    if (!value || typeof value !== 'object') return;
    if (value.type === 'VariableDeclarator') {
      const call = value.init;
      if (call?.type === 'CallExpression' && call.callee?.property?.value === 'useLoaderData')
        loaderName = getPatternName(value.id);
      if (call?.type === 'CallExpression' && call.callee?.value === 'useState') {
        const argument = call.arguments?.[0]?.expression ?? call.arguments?.[0];
        if (getNodeName(argument) === loaderName)
          stateName = getPatternName(value.id?.elements?.[0]);
        const name = getPatternName(value.id?.elements?.[0]);
        if (name) stateNames.push(name);
      }
    }
    for (const child of Object.values(value)) {
      if (Array.isArray(child)) child.forEach(visit);
      else visit(child);
    }
  };
  visit(ast);
  const slot = stateNames.indexOf(stateName ?? '');
  return slot >= 0 && slot < graph.stateSlots.length ? slot : undefined;
}

type RouteComponent = { moduleId: string; name: string };
type RouteDefinition = {
  path: string;
  component: RouteComponent;
  pendingComponent?: RouteComponent;
  errorComponent?: RouteComponent;
  loader?: { source: string; moduleId: string };
};

/** Reuse the event-action lowering pipeline for a route loader. This keeps
 * fetch/error semantics identical without adding a second JS interpreter. */
function compileRouteLoader(
  loader: { source: string; moduleId: string },
  modules: SourceGraphModule[],
  options: { rootDir: string; repoRootDir: string; mode?: CompileOptions['mode'] },
  revision: string,
) {
  const source = `${modules.find((module) => module.id === loader.moduleId)?.source ?? ''}\nexport function PlecRouteLoader() { return <button onClick={${loader.source}} />; }`;
  const result = compile(source, {
    mode: options.mode ?? 'strict',
    rootComponent: 'PlecRouteLoader',
    moduleId: loader.moduleId,
    modules: modules.map((module) => module.id === loader.moduleId ? { ...module, source } : module),
    applicationRevision: revision,
  });
  const action = (result.ir as any).actions[0];
  if (!action) throw new Error(`Route loader in ${loader.moduleId} did not lower to an action.`);
  validateRouteLoaderAction(action, loader.moduleId);
  return { ...action, parameterSlots: [] };
}

/** Router policy is intentionally stricter than the generic typed action VM. */
export function validateRouteLoaderAction(action: any, moduleId = 'route loader') {
  const requests = (action.instructions ?? []).filter(
    (instruction: any) => instruction.op === 'capabilityRequest' && instruction.capability === 'fetch',
  );
  if (requests.length !== 1)
    throw new Error(`Route loader in ${moduleId} requires exactly one terminal fetch capability request.`);
  const request = requests[0];
  const length = action.instructions.length;
  if (
    !Number.isInteger(request.successPc) ||
    !Number.isInteger(request.failurePc) ||
    request.successPc < 0 ||
    request.failurePc < 0 ||
    request.successPc >= length ||
    request.failurePc >= length ||
    (request.finallyPc !== undefined && (!Number.isInteger(request.finallyPc) || request.finallyPc < 0 || request.finallyPc >= length))
  )
    throw new Error(`Route loader in ${moduleId} has invalid terminal fetch continuations.`);
}

function importBindings(module: SourceGraphModule): Map<string, string> {
  const ast: any = parseSync(module.source, { syntax: 'typescript', tsx: true, target: 'es2022' });
  const result = new Map<string, string>();
  for (const statement of ast.body ?? []) {
    if (statement.type !== 'ImportDeclaration') continue;
    const source = statement.source?.value;
    if (!source) continue;
    const moduleId = logicalModuleId(module.id, source);
    for (const specifier of statement.specifiers ?? []) {
      const local = specifier.local?.value ?? specifier.local?.id?.value;
      if (local) result.set(local, moduleId);
    }
  }
  return result;
}

function routeTreeBindings(source: string): string[] {
  const ast: any = parseSync(source, { syntax: 'typescript', tsx: true, target: 'es2022' });
  let root: string | undefined;
  let children: string[] = [];
  const visit = (value: any): void => {
    if (!value || typeof value !== 'object') return;
    if (
      value.type === 'CallExpression' &&
      value.callee?.type === 'MemberExpression' &&
      value.callee.property?.value === 'addChildren'
    ) {
      root = value.callee.object?.value;
      const array = value.arguments?.[0]?.expression ?? value.arguments?.[0];
      children = (array?.elements ?? []).map((item: any) =>
        (item?.expression ?? item)?.value,
      ).filter(Boolean);
    }
    for (const child of Object.values(value)) {
      if (Array.isArray(child)) child.forEach(visit);
      else visit(child);
    }
  };
  visit(ast);
  return root ? [root, ...children] : [];
}

function parseRouteDefinition(
  module: SourceGraphModule,
  byId: Map<string, SourceGraphModule>,
): RouteDefinition {
  const ast: any = parseSync(module.source, { syntax: 'typescript', tsx: true, target: 'es2022' });
  const imports = importBindings(module);
  let options: any;
  const visit = (value: any): void => {
    if (!value || typeof value !== 'object' || options) return;
    if (
      value.type === 'CallExpression' &&
      ['createRootRoute', 'createRoute'].includes(value.callee?.value) &&
      (value.arguments?.[0]?.expression ?? value.arguments?.[0])?.type === 'ObjectExpression'
    ) options = value.arguments[0]?.expression ?? value.arguments[0];
    for (const child of Object.values(value)) {
      if (Array.isArray(child)) child.forEach(visit);
      else visit(child);
    }
  };
  visit(ast);
  if (!options) throw new Error(`Route module ${module.id} has no createRoute definition.`);
  const property = (name: string) => (options.properties ?? []).find(
    (entry: any) => (entry.key?.value ?? entry.key?.name) === name,
  )?.value;
  const component = (name: string): RouteComponent | undefined => {
    const value = property(name);
    const local = value?.value;
    const moduleId = local && imports.get(local);
    if (!local) return undefined;
    // A component declared directly in the route module is also valid.
    return { moduleId: moduleId ?? module.id, name: local };
  };
  const resolved = component('component');
  if (!resolved) throw new Error(`Route module ${module.id} has no component export.`);
  if (!byId.has(resolved.moduleId))
    throw new Error(`Component source is unavailable: ${resolved.moduleId}#${resolved.name}.`);
  return {
    path: property('path')?.value ?? '',
    component: resolved,
    pendingComponent: component('pendingComponent'),
    errorComponent: component('errorComponent'),
    ...(property('loader')?.span
      ? { loader: { source: module.source.slice(property('loader').span.start - 1, property('loader').span.end - 1), moduleId: module.id } }
      : {}),
  };
}

function logicalModuleId(from: string, specifier: string): string {
  if (!specifier.startsWith('.')) return specifier;
  const base = from.split('/');
  if (/\.(?:[cm]?[jt]sx?)$/.test(from)) base.pop();
  for (const part of specifier.split('/')) {
    if (!part || part === '.') continue;
    if (part === '..') base.pop();
    else base.push(part);
  }
  const resolved = base.join('/');
  return /\.(?:[cm]?[jt]sx?)$/.test(resolved)
    ? resolved
    : `${resolved}.tsx`;
}
function getNodeName(value: any): string | undefined {
  return value?.value ?? value?.name ?? value?.id?.value;
}
function getPatternName(value: any): string | undefined {
  return value?.value ?? value?.name ?? value?.id?.value;
}
function resolveDependencyModule(
  specifier: string,
  fromFile: string,
): string | null {
  if (specifier.startsWith('.') || specifier.startsWith('node:'))
    return null;
  try {
    const resolved = createRequire(fromFile).resolve(specifier);
    const esm = resolved.replace(/\.js$/, '.mjs');
    return existsSync(esm) ? esm : resolved;
  } catch {
    return null;
  }
}
async function resolveWorkspaceModule(
  specifier: string,
  repoRootDir: string,
): Promise<string | null> {
  const match = specifier.match(
    /^(?:@wasm-runtime\/)?([a-z][a-z0-9-]*)(\/.*)?$/,
  );
  if (!match) return null;
  try {
    const packageDir = path.resolve(repoRootDir, 'packages', match[1]!);
    if (!match[2]) {
      for (const sourceEntry of ['src/index.tsx', 'src/index.ts']) {
        const candidate = path.join(packageDir, sourceEntry);
        if (existsSync(candidate)) return candidate;
      }
    }
    const manifest = JSON.parse(
      await readFile(path.join(packageDir, 'package.json'), 'utf8'),
    ) as { exports?: Record<string, ExportTarget> };
    const requested = `.${match[2] ?? ''}`;
    for (const [pattern, target] of Object.entries(
      manifest.exports ?? {},
    )) {
      const resolved = resolveExportTarget(target);
      if (pattern === requested && resolved)
        return resolveWorkspaceSource(packageDir, resolved);
      const wildcard = pattern.indexOf('*');
      if (wildcard < 0) continue;
      const prefix = pattern.slice(0, wildcard);
      const suffix = pattern.slice(wildcard + 1);
      if (!requested.startsWith(prefix) || !requested.endsWith(suffix))
        continue;
      if (!resolved) continue;
      return resolveWorkspaceSource(
        packageDir,
        resolved.replace(
          '*',
          requested.slice(prefix.length, requested.length - suffix.length),
        ),
      );
    }
  } catch (error: any) {
    if (error?.code !== 'ENOENT') throw error;
  }
  return null;
}

function resolveWorkspaceSource(packageDir: string, target: string): string {
  const published = path.resolve(packageDir, target);
  const source = path.resolve(
    packageDir,
    target
      .replace(/^\.\/dist\//, './src/')
      .replace(/\.d\.ts$/, '.ts')
      .replace(/\.js$/, '.ts'),
  );
  return existsSync(source) ? source : published;
}

type ExportTarget = string | ConditionalExportTarget;
interface ConditionalExportTarget {
  [condition: string]: ExportTarget;
}

/** Package exports commonly select a source-compatible entry through
 * `import`/`default` conditions. The compiler only needs a deterministic
 * module to inspect; it never executes the target. */
function resolveExportTarget(target: ExportTarget): string | undefined {
  if (typeof target === 'string') return target;
  for (const condition of ['source', 'import', 'default', 'types']) {
    const candidate = target[condition];
    if (!candidate) continue;
    const resolved = resolveExportTarget(candidate);
    if (resolved) return resolved;
  }
  return undefined;
}
