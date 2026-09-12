import { isObjectRecord as isRecord } from "../../utils/record";
import type { SqlValue } from "#types";
import { serializePrimitive } from "./serialize-primitive";

export function serializeSqlValue(value: SqlValue): unknown {
  if (Array.isArray(value)) {
    return value.map(serializePrimitive);
  }

  if (typeof value === "bigint" || value instanceof Date) {
    return serializePrimitive(value);
  }

  if (isRecord(value)) {
    if (
      "type" in value &&
      typeof (value as { type?: unknown }).type === "string"
    ) {
      return value;
    }

    return "values" in value
      ? {
          text: value.text,
          raw: value.raw,
          values: value.values.map(serializePrimitive),
        }
      : value;
  }

  return serializePrimitive(value);
}
