import { isObjectRecord as isRecord } from "../../utils/record";
import type { Primitive } from "#types";
import { revivePrimitive } from "./revive-primitive";

export function revivePrimitiveList(values: unknown[]): Primitive[] {
  for (const value of values) {
    if (
      isRecord(value) &&
      "__kind" in value &&
      ((value as { __kind?: string }).__kind === "date" ||
        (value as { __kind?: string }).__kind === "bigint")
    ) {
      return values.map(revivePrimitive);
    }
  }

  return values as Primitive[];
}
