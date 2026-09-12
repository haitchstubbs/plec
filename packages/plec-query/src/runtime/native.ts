import { browserFallbackBinding } from "./browser-fallback";
import type { RuntimeBinding } from "./types";

function withMissingBindingsFilled(binding: RuntimeBinding): RuntimeBinding {
  return {
    ...binding,
    compilePostgres:
      binding.compilePostgres ?? browserFallbackBinding.compilePostgres,
    compileQuery: binding.compileQuery ?? browserFallbackBinding.compileQuery,
    overClause: binding.overClause ?? browserFallbackBinding.overClause,
  };
}

export function loadNativeBinding(): RuntimeBinding {
  const nodeProcess = globalThis.process;

  if (
    !nodeProcess?.versions?.node ||
    typeof nodeProcess.getBuiltinModule !== "function"
  ) {
    throw new Error(
      "@haitchstack/query native binding is only available in a Node runtime.",
    );
  }

  const moduleModule = nodeProcess.getBuiltinModule(
    "node:module",
  ) as typeof import("node:module");
  const fsModule = nodeProcess.getBuiltinModule(
    "node:fs",
  ) as typeof import("node:fs");
  const pathModule = nodeProcess.getBuiltinModule(
    "node:path",
  ) as typeof import("node:path");
  const urlModule = nodeProcess.getBuiltinModule(
    "node:url",
  ) as typeof import("node:url");

  const require = moduleModule.createRequire(import.meta.url);
  const explicitPath = nodeProcess.env.NODE_QUERY_NATIVE_BINDING;
  const allowBrowserFallback =
    nodeProcess.env.NODE_QUERY_ALLOW_BROWSER_FALLBACK === "1" ||
    nodeProcess.env.NODE_QUERY_RUNTIME_FALLBACK === "browser";

  if (explicitPath) {
    return withMissingBindingsFilled(require(explicitPath) as RuntimeBinding);
  }

  const moduleDir = pathModule.dirname(
    urlModule.fileURLToPath(import.meta.url),
  );
  const nativeDir = pathModule.resolve(moduleDir, "../../native");
  const candidates = [
    pathModule.resolve(nativeDir, "query.node"),
    pathModule.resolve(
      moduleDir,
      "../../../../crates/query/target/release/query_node.node",
    ),
  ];

  if (fsModule.existsSync(nativeDir)) {
    const nativeEntries = fsModule
      .readdirSync(nativeDir)
      .filter((e) => e.endsWith(".node"))
      .sort((a, b) => {
        const mtimeA = fsModule.statSync(
          pathModule.resolve(nativeDir, a),
        ).mtimeMs;
        const mtimeB = fsModule.statSync(
          pathModule.resolve(nativeDir, b),
        ).mtimeMs;
        return mtimeA - mtimeB; // oldest first → unshift makes newest highest priority
      });
    for (const entry of nativeEntries) {
      candidates.unshift(pathModule.resolve(nativeDir, entry));
    }
  }

  const failures: string[] = [];
  for (const candidate of candidates) {
    try {
      return withMissingBindingsFilled(require(candidate) as RuntimeBinding);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      failures.push(`${candidate}: ${message}`);
      // Try the next generated/native candidate until one resolves.
    }
  }

  if (allowBrowserFallback) {
    return browserFallbackBinding;
  }

  throw new Error(
    [
      "@haitchstack/query native binding could not be loaded in Node.",
      "Set NODE_QUERY_ALLOW_BROWSER_FALLBACK=1 only for explicit test/browser fallback mode.",
      "Attempted native candidates:",
      ...failures.map((failure) => `- ${failure}`),
    ].join("\n"),
  );
}
