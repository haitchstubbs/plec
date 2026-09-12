import type { PredicateInput, SqlQuery, SqlValue } from "#types";
import { serializeSqlValue } from "../../runtime/bridge";
import { makeExprNode } from "./make-expr-node";

export function toExprNodeFromPredicate(cond: PredicateInput): SqlQuery {
  if (typeof cond === "string")
    return makeExprNode({ type: "raw", text: cond });
  const obj = cond as Record<string, unknown>;
  if ("type" in obj && typeof obj.type === "string") {
    return cond as SqlQuery;
  }
  const q = cond as SqlQuery;
  return makeExprNode({
    type: "query",
    query: {
      text: q.text,
      raw: q.raw,
      values: q.values.map((v) => serializeSqlValue(v as SqlValue)),
    },
  });
}
