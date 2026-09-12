import {
  clearWasmBinding,
  setWasmBinding,
  type WireAliasedQuery,
  type WireQuery,
} from "../runtime/bridge";
import { browserFallbackBinding } from "../runtime/browser-fallback";

let initialized = false;

type GeneratedWasmBinding = {
  default?: () => Promise<unknown> | unknown;
  // SQL primitives
  identifier: (parts: unknown) => unknown;
  raw: (text: string) => unknown;
  join: (items: unknown[], separator?: string | null) => WireQuery;
  sql: (strings: string[], exprs: unknown[]) => WireQuery;
  ref_identifier: (parts: unknown) => unknown;
  compile_postgres: (query: WireQuery) => WireQuery;
  compile_query: (query: WireQuery, dialect: string) => WireQuery;
  // Expressions
  cmp: (left: unknown, operator: string, right: unknown) => WireQuery;
  is_null: (value: unknown) => WireQuery;
  is_not_null: (value: unknown) => WireQuery;
  in_array: (value: unknown, items: unknown[]) => WireQuery;
  not_in_array: (value: unknown, items: unknown[]) => WireQuery;
  between: (value: unknown, lower: unknown, upper: unknown) => WireQuery;
  not_between: (value: unknown, lower: unknown, upper: unknown) => WireQuery;
  like_sql: (value: unknown, pattern: unknown) => WireQuery;
  not_like_sql: (value: unknown, pattern: unknown) => WireQuery;
  exists_sql: (query: WireQuery) => WireQuery;
  not_exists_sql: (query: WireQuery) => WireQuery;
  and: (conditions: unknown[]) => WireQuery;
  or: (conditions: unknown[]) => WireQuery;
  fn_call: (name: string, args: unknown[]) => WireQuery;
  scalar_case: (branches: unknown[], elseVal: unknown) => WireQuery;
  arith_binary: (left: unknown, operator: string, right: unknown) => WireQuery;
  over_clause: (
    query: WireQuery,
    partitionBy: unknown[],
    orderBy: unknown[],
    dialect: string,
  ) => WireQuery;
  // Builder utility
  builder_new: (dialect?: string | null) => string;
  builder_clone: (handle: string) => string;
  builder_drop: (handle: string) => void;
  builder_clear: (handle: string) => string;
  // Builder FROM
  builder_from_table: (handle: string, table: string) => string;
  builder_from_table_alias: (
    handle: string,
    table: string,
    alias: string,
  ) => string;
  builder_from_subquery: (
    handle: string,
    alias: string,
    queryJson: string,
  ) => string;
  // Builder DISTINCT
  builder_distinct: (handle: string) => string;
  builder_distinct_on_columns: (handle: string, colsJson: string) => string;
  builder_distinct_on_exprs: (handle: string, exprsJson: string) => string;
  // Builder SELECT
  builder_select_columns: (handle: string, colsJson: string) => string;
  builder_select_aliased: (handle: string, aliasesJson: string) => string;
  builder_select_fragment: (
    handle: string,
    fragmentJson: string,
    selectedColsJson: string,
  ) => string;
  // Builder JOIN
  builder_join_table: (
    handle: string,
    joinType: string,
    table: string,
  ) => string;
  builder_join_table_alias: (
    handle: string,
    joinType: string,
    table: string,
    alias: string,
  ) => string;
  builder_join_subquery: (
    handle: string,
    joinType: string,
    alias: string,
    queryJson: string,
  ) => string;
  builder_on: (handle: string, predJson: string) => string;
  builder_and_on: (handle: string, predJson: string) => string;
  builder_or_on: (handle: string, predJson: string) => string;
  builder_using_columns: (handle: string, colsJson: string) => string;
  builder_on_columns: (handle: string, pairsJson: string) => string;
  // Builder WHERE / HAVING
  builder_where: (handle: string, predJson: string) => string;
  builder_and_where: (handle: string, predJson: string) => string;
  builder_or_where: (handle: string, predJson: string) => string;
  builder_having: (handle: string, predJson: string) => string;
  builder_and_having: (handle: string, predJson: string) => string;
  builder_or_having: (handle: string, predJson: string) => string;
  // Builder GROUP BY
  builder_group_by_columns: (handle: string, colsJson: string) => string;
  // Builder ORDER BY
  builder_order_by_column: (
    handle: string,
    col: string,
    direction?: string | null,
    null_order?: string | null,
  ) => string;
  builder_order_by_columns: (handle: string, colsJson: string) => string;
  // Builder LIMIT / OFFSET
  builder_limit: (handle: string, count: number) => string;
  builder_offset: (handle: string, count: number) => string;
  builder_for_update: (handle: string) => string;
  builder_for_share: (handle: string) => string;
  builder_no_wait: (handle: string) => string;
  builder_skip_locked: (handle: string) => string;
  // Builder COMPOUND
  builder_union: (handle: string, queryJson: string) => string;
  builder_union_all: (handle: string, queryJson: string) => string;
  builder_intersect: (handle: string, queryJson: string) => string;
  builder_except: (handle: string, queryJson: string) => string;
  builder_union_handle: (handle: string, rhsHandle: string) => string;
  builder_union_all_handle: (handle: string, rhsHandle: string) => string;
  builder_intersect_handle: (handle: string, rhsHandle: string) => string;
  builder_except_handle: (handle: string, rhsHandle: string) => string;
  // Builder CTE
  builder_with: (handle: string, name: string, queryJson: string) => string;
  builder_with_recursive: (
    handle: string,
    name: string,
    queryJson: string,
  ) => string;
  builder_with_handle: (
    handle: string,
    name: string,
    rhsHandle: string,
  ) => string;
  builder_with_recursive_handle: (
    handle: string,
    name: string,
    rhsHandle: string,
  ) => string;
  builder_from_subquery_handle: (
    handle: string,
    alias: string,
    rhsHandle: string,
  ) => string;
  builder_join_subquery_handle: (
    handle: string,
    joinType: string,
    alias: string,
    rhsHandle: string,
  ) => string;
  // Builder INSERT
  builder_insert_into: (handle: string, table: string) => string;
  builder_columns: (handle: string, colsJson: string) => string;
  builder_values_insert: (handle: string, rowsJson: string) => string;
  builder_insert_select: (handle: string, queryJson: string) => string;
  builder_insert_select_handle: (handle: string, rhsHandle: string) => string;
  builder_on_conflict_columns: (handle: string, colsJson: string) => string;
  builder_on_conflict_constraint: (
    handle: string,
    constraint: string,
  ) => string;
  builder_do_nothing: (handle: string) => string;
  builder_do_update_set: (handle: string, assignmentsJson: string) => string;
  builder_conflict_where: (handle: string, predJson: string) => string;
  builder_returning_columns: (handle: string, colsJson: string) => string;
  builder_returning_aliased: (handle: string, aliasesJson: string) => string;
  builder_returning_fragment: (
    handle: string,
    fragmentJson: string,
    selectedColsJson: string,
  ) => string;
  // Builder UPDATE
  builder_update: (handle: string, table: string) => string;
  builder_set: (handle: string, assignmentsJson: string) => string;
  // Builder DELETE
  builder_delete_from: (handle: string, table: string) => string;
  // Builder output
  builder_query: (handle: string) => WireQuery;
  builder_text: (handle: string) => string;
  builder_raw: (handle: string) => string;
  builder_values: (handle: string) => unknown[];
  builder_selected_columns: (handle: string) => string[];
  builder_insert_columns: (handle: string) => string[];
  builder_as: (handle: string, alias: string) => WireAliasedQuery;
  builder_compile_bundle?: (handle: string) => {
    canonical_hash: string;
    text: string;
    raw: string;
    values: unknown[];
  };
  builder_canonical_ir_hash?: (handle: string) => string;
  builder_apply_ops: (handle: string, opsJson: string) => string;
  builder_apply_ops_binary?: (handle: string, payload: Uint8Array) => string;
};

export async function initNodeQueryWasm(): Promise<void> {
  try {
    const generatedBinding =
      (await import("../generated/wasm/query_wasm.js")) as unknown as GeneratedWasmBinding;
    if (typeof generatedBinding.default === "function") {
      await generatedBinding.default();
    }
    setWasmBinding({
      // SQL primitives
      identifier: generatedBinding.identifier,
      raw: generatedBinding.raw,
      join: generatedBinding.join,
      sql: generatedBinding.sql,
      refIdentifier: generatedBinding.ref_identifier,
      compilePostgres: generatedBinding.compile_postgres,
      compileQuery: generatedBinding.compile_query,
      // Expressions
      cmp: generatedBinding.cmp,
      isNull: generatedBinding.is_null,
      isNotNull: generatedBinding.is_not_null,
      inArray: generatedBinding.in_array,
      notInArray: generatedBinding.not_in_array,
      between: generatedBinding.between,
      notBetween: generatedBinding.not_between,
      likeSql: generatedBinding.like_sql,
      notLikeSql: generatedBinding.not_like_sql,
      existsSql: generatedBinding.exists_sql,
      notExistsSql: generatedBinding.not_exists_sql,
      and: generatedBinding.and,
      or: generatedBinding.or,
      fnCall: generatedBinding.fn_call,
      scalarCase: generatedBinding.scalar_case,
      arithBinary: generatedBinding.arith_binary,
      overClause: generatedBinding.over_clause,
      // Builder utility
      builderNew: generatedBinding.builder_new,
      builderClone: generatedBinding.builder_clone,
      builderDrop: generatedBinding.builder_drop,
      builderClear: generatedBinding.builder_clear,
      // Builder FROM
      builderFromTable: generatedBinding.builder_from_table,
      builderFromTableAlias: generatedBinding.builder_from_table_alias,
      builderFromSubquery: generatedBinding.builder_from_subquery,
      // Builder DISTINCT
      builderDistinct: generatedBinding.builder_distinct,
      builderDistinctOnColumns: generatedBinding.builder_distinct_on_columns,
      builderDistinctOnExprs: generatedBinding.builder_distinct_on_exprs,
      // Builder SELECT
      builderSelectColumns: generatedBinding.builder_select_columns,
      builderSelectAliased: generatedBinding.builder_select_aliased,
      builderSelectFragment: generatedBinding.builder_select_fragment,
      // Builder JOIN
      builderJoinTable: generatedBinding.builder_join_table,
      builderJoinTableAlias: generatedBinding.builder_join_table_alias,
      builderJoinSubquery: generatedBinding.builder_join_subquery,
      builderOn: generatedBinding.builder_on,
      builderAndOn: generatedBinding.builder_and_on,
      builderOrOn: generatedBinding.builder_or_on,
      builderUsingColumns: generatedBinding.builder_using_columns,
      builderOnColumns: generatedBinding.builder_on_columns,
      // Builder WHERE / HAVING
      builderWhere: generatedBinding.builder_where,
      builderAndWhere: generatedBinding.builder_and_where,
      builderOrWhere: generatedBinding.builder_or_where,
      builderHaving: generatedBinding.builder_having,
      builderAndHaving: generatedBinding.builder_and_having,
      builderOrHaving: generatedBinding.builder_or_having,
      // Builder GROUP BY
      builderGroupByColumns: generatedBinding.builder_group_by_columns,
      // Builder ORDER BY
      builderOrderByColumn: generatedBinding.builder_order_by_column,
      builderOrderByColumns: generatedBinding.builder_order_by_columns,
      // Builder LIMIT / OFFSET
      builderLimit: generatedBinding.builder_limit,
      builderOffset: generatedBinding.builder_offset,
      builderForUpdate: generatedBinding.builder_for_update,
      builderForShare: generatedBinding.builder_for_share,
      builderNoWait: generatedBinding.builder_no_wait,
      builderSkipLocked: generatedBinding.builder_skip_locked,
      // Builder COMPOUND
      builderUnion: generatedBinding.builder_union,
      builderUnionAll: generatedBinding.builder_union_all,
      builderIntersect: generatedBinding.builder_intersect,
      builderExcept: generatedBinding.builder_except,
      builderUnionHandle: generatedBinding.builder_union_handle,
      builderUnionAllHandle: generatedBinding.builder_union_all_handle,
      builderIntersectHandle: generatedBinding.builder_intersect_handle,
      builderExceptHandle: generatedBinding.builder_except_handle,
      // Builder CTE
      builderWith: generatedBinding.builder_with,
      builderWithRecursive: generatedBinding.builder_with_recursive,
      builderWithHandle: generatedBinding.builder_with_handle,
      builderWithRecursiveHandle:
        generatedBinding.builder_with_recursive_handle,
      builderFromSubqueryHandle: generatedBinding.builder_from_subquery_handle,
      builderJoinSubqueryHandle: generatedBinding.builder_join_subquery_handle,
      // Builder INSERT
      builderInsertInto: generatedBinding.builder_insert_into,
      builderColumns: generatedBinding.builder_columns,
      builderValuesInsert: generatedBinding.builder_values_insert,
      builderInsertSelect: generatedBinding.builder_insert_select,
      builderInsertSelectHandle: generatedBinding.builder_insert_select_handle,
      builderOnConflictColumns: generatedBinding.builder_on_conflict_columns,
      builderOnConflictConstraint:
        generatedBinding.builder_on_conflict_constraint,
      builderDoNothing: generatedBinding.builder_do_nothing,
      builderDoUpdateSet: generatedBinding.builder_do_update_set,
      builderConflictWhere: generatedBinding.builder_conflict_where,
      builderReturningColumns: generatedBinding.builder_returning_columns,
      builderReturningAliased: generatedBinding.builder_returning_aliased,
      builderReturningFragment: generatedBinding.builder_returning_fragment,
      // Builder UPDATE
      builderUpdate: generatedBinding.builder_update,
      builderSet: generatedBinding.builder_set,
      // Builder DELETE
      builderDeleteFrom: generatedBinding.builder_delete_from,
      // Builder output
      builderQuery: generatedBinding.builder_query,
      builderText: generatedBinding.builder_text,
      builderRaw: generatedBinding.builder_raw,
      builderValues: generatedBinding.builder_values,
      builderSelectedColumns: generatedBinding.builder_selected_columns,
      builderInsertColumns: generatedBinding.builder_insert_columns,
      builderAs: generatedBinding.builder_as,
      builderCompileBundle: generatedBinding.builder_compile_bundle,
      builderCanonicalIrHash: generatedBinding.builder_canonical_ir_hash,
      builderApplyOps: generatedBinding.builder_apply_ops,
      builderApplyOpsBinary: generatedBinding.builder_apply_ops_binary,
    });
    initialized = true;
    return;
  } catch {
    // Fall back to the native or browser runtime below when wasm init is unavailable.
  }

  if (typeof process !== "undefined" && process.versions?.node) {
    const { loadNativeBinding } = await import("../runtime/native");
    setWasmBinding(loadNativeBinding());
    initialized = true;
    return;
  }

  setWasmBinding(browserFallbackBinding);
  initialized = true;
}

export function assertNodeQueryWasmInitialized(): void {
  if (!initialized) {
    throw new Error(
      "@haitchstack/query/wasm requires initNodeQueryWasm() before constructing Database.",
    );
  }
}

export function resetNodeQueryWasmForTests(): void {
  initialized = false;
  clearWasmBinding();
}
