import type { SqlQuery } from "#types";
import type { WireQuery } from "../types";
import { serializePrimitive } from "./serialize-primitive";

export function serializeQuery(query: SqlQuery): WireQuery {
  return {
    text: query.text,
    raw: query.raw,
    values: query.values.map(serializePrimitive),
  };
}
