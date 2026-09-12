import type { Primitive } from "#types";
import { revivePrimitiveList } from "./revive-primitive-list";

export function reviveCompileBundle<T extends { values: unknown[] }>(
  bundle: T,
): T & {
  values: Primitive[];
} {
  const values = revivePrimitiveList(bundle.values);
  if (values === bundle.values) {
    return bundle as T & { values: Primitive[] };
  }

  return {
    ...bundle,
    values,
  };
}
