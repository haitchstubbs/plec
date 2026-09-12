import type { Primitive } from "#types";

export function serializePrimitive(value: Primitive): unknown {
  if (typeof value === "bigint") {
    return { __kind: "bigint", value: value.toString() };
  }

  if (value instanceof Date) {
    return { __kind: "date", value: value.toISOString() };
  }

  return value;
}
