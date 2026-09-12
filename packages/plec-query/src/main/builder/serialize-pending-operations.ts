import type { CompactPendingOp, PendingOp } from "#types";

function tuple<const T extends readonly unknown[]>(...value: T): T {
  return value;
}

export function serializePendingOp(op: PendingOp): CompactPendingOp {
  switch (op.op) {
    case "where":
      return tuple("w", op.pred);
    case "distinct":
      return tuple("d");
    case "distinctOnColumns":
      return tuple("dc", op.cols);
    case "distinctOnExprs":
      return tuple("de", op.exprs);
    case "andWhere":
      return tuple("aw", op.pred);
    case "orWhere":
      return tuple("ow", op.pred);
    case "having":
      return tuple("h", op.pred);
    case "andHaving":
      return tuple("ah", op.pred);
    case "orHaving":
      return tuple("oh", op.pred);
    case "fromTable":
      return tuple("ft", op.table);
    case "fromTableAlias":
      return tuple("fta", op.table, op.alias);
    case "joinTable":
      return tuple("jt", op.joinType, op.table);
    case "joinTableAlias":
      return tuple("jta", op.joinType, op.table, op.alias);
    case "on":
      return tuple("on", op.pred);
    case "andOn":
      return tuple("aon", op.pred);
    case "orOn":
      return tuple("oon", op.pred);
    case "usingColumns":
      return tuple("uc", op.cols);
    case "onColumns":
      return tuple("oc", op.pairs);
    case "withBuilder":
    case "withRecursiveBuilder":
    case "unionBuilder":
    case "unionAllBuilder":
    case "intersectBuilder":
    case "exceptBuilder":
    case "fromSubqueryBuilder":
    case "joinSubqueryBuilder":
      throw new Error(
        `Nested builder op '${op.op}' must be resolved before serialization.`,
      );
    case "withHandle":
      return tuple("wh", op.name, op.rhsHandle);
    case "withQuery":
      return tuple("wq", op.name, op.query);
    case "withRecursiveHandle":
      return tuple("wrh", op.name, op.rhsHandle);
    case "withRecursiveQuery":
      if (Array.isArray(op.columns) && op.columns.length > 0) {
        return {
          op: "withRecursiveQuery",
          name: op.name,
          query: op.query,
          columns: op.columns,
        };
      }
      return tuple("wrq", op.name, op.query);
    case "insertInto":
      return tuple("ii", op.table);
    case "insertColumns":
      return tuple("ic", op.cols);
    case "valuesInsert":
      return tuple("vi", op.columnNames, op.rows);
    case "update":
      return tuple("upd", op.table);
    case "deleteFrom":
      return tuple("del", op.table);
    case "set":
      return tuple("set", op.assignments);
    case "onConflictColumns":
      return tuple("ict", op.cols);
    case "onConflictConstraint":
      return tuple("icn", op.name);
    case "doNothing":
      return tuple("idn");
    case "doUpdateSet":
      return tuple("idu", op.assignments);
    case "conflictWhere":
      return tuple("icw", op.pred);
    case "selectAliased":
      return tuple("sa", op.entries);
    case "selectColumns":
      return tuple("sc", op.cols);
    case "returningAliased":
      return tuple("ra", op.entries);
    case "returningColumns":
      return tuple("rc", op.cols);
    case "orderBy":
      return tuple("ob", op.col, op.direction ?? null, op.nullOrder ?? null);
    case "orderByColumns":
      return tuple("obc", op.cols);
    case "groupBy":
      return tuple("gb", op.cols);
    case "limit":
      return tuple("l", op.count);
    case "offset":
      return tuple("o", op.count);
    case "forUpdate":
      return tuple("fu");
    case "forShare":
      return tuple("fs");
    case "noWait":
      return tuple("nw");
    case "skipLocked":
      return tuple("sl");
    case "unionHandle":
      return tuple("unh", op.rhsHandle);
    case "union":
      return tuple("un", op.query);
    case "unionAllHandle":
      return tuple("uah", op.rhsHandle);
    case "unionAll":
      return tuple("ua", op.query);
    case "intersectHandle":
      return tuple("ixh", op.rhsHandle);
    case "intersect":
      return tuple("ix", op.query);
    case "exceptHandle":
      return tuple("exh", op.rhsHandle);
    case "except":
      return tuple("ex", op.query);
    case "fromSubqueryHandle":
      return tuple("fsqh", op.alias, op.rhsHandle);
    case "fromSubquery":
      return tuple("fsq", op.alias, op.query);
    case "joinSubqueryHandle":
      return tuple("jsqh", op.joinType, op.alias, op.rhsHandle);
    case "joinSubquery":
      return tuple("jsq", op.joinType, op.alias, op.query);
    case "insertSelectHandle":
      return tuple("ish", op.rhsHandle);
    case "insertSelect":
      return tuple("is", op.query);
    default:
      return op satisfies never;
  }
}
