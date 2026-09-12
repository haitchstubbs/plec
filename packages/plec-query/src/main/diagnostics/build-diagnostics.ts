import type { NodeQueryBuildDiagnostics } from "../types";

export function createNodeQueryBuildDiagnosticsSample(): NodeQueryBuildDiagnostics {
  return {
    directMutationMs: 0,
    cloneWithOwnedHandleMs: 0,
    callbackResolveMs: 0,
    builderContextCreateMs: 0,
    exprNodeBuildMs: 0,
    predicateBuildMs: 0,
    jsonSerializeMs: 0,
    runtimeCallMs: 0,
    runtimeSourceMs: 0,
    runtimeSelectMs: 0,
    runtimePredicateMs: 0,
    runtimeCteMs: 0,
    runtimeOrderMs: 0,
    runtimePaginationMs: 0,
    directMutationCount: 0,
    runtimeCallCount: 0,
    builderContextCacheHits: 0,
    builderContextCacheMisses: 0,
    exprNodeCount: 0,
    predicateBuildCount: 0,
    jsonSerializeCount: 0,
  };
}
