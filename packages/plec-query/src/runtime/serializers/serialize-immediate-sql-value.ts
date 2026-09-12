import { isObjectRecord as isRecord } from "../../utils/record";
import type { SqlQuery, SqlValue } from "#types";
import { serializeQuery } from "./serialize-query";
import { serializeSqlValue } from "./serialize-sql-value";

export function serializeImmediateSqlValue(value: SqlValue): unknown {
  if (
    isRecord(value) &&
    "type" in value &&
    typeof (value as { type?: unknown }).type === "string"
  ) {
    return serializeQuery(value as SqlQuery);
  }
  return serializeSqlValue(value);
}
