import type { BuilderContext } from "#types";

export const sharedBuilderContextCache = new Map<
  string,
  BuilderContext<string>
>();
