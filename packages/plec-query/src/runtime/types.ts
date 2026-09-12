// Wire types — raw JS objects crossing the FFI boundary before client-side revival
export type DeferredQueryNode = Record<string, unknown>;

export type WireQuery = {
  text: string;
  raw: string;
  values: unknown[];
};

export type WireAliasedQuery = {
  __kind: string;
  alias: string;
  __handle?: string;
  query: WireQuery;
  text: string;
  raw: string;
  values: unknown[];
  selectedColumns: string[];
};

type Fn<Args extends unknown[] = [], Return = unknown> = (
  ...args: Args
) => Return;

type Method<
  Name extends string,
  Args extends unknown[] = [],
  Return = unknown,
> = {
  [K in Name]: Fn<Args, Return>;
};

type BuilderMethod<
  Name extends string,
  Args extends unknown[] = [],
  Return = string,
> = Method<Name, [handle: string, ...args: Args], Return>;

type OptionalMethod<Name extends string, Args extends unknown[], Return> = {
  [K in Name]?: Fn<Args, Return>;
};

// ─── SQL primitives ──────────────────────────────────────────────────────────

type SqlPrimitives = Method<"identifier", [parts: string[]]> &
  Method<"raw", [text: string]> &
  Method<"join", [items: unknown[], separator?: string], WireQuery> &
  Method<"sql", [strings: string[], exprs: unknown[]], WireQuery> &
  Method<"refIdentifier", [parts: string[]]> &
  Method<"compilePostgres", [query: WireQuery], WireQuery> &
  Method<"compileQuery", [query: WireQuery, dialect: string], WireQuery>;

// ─── Expressions ─────────────────────────────────────────────────────────────

type Expressions = Method<
  "cmp",
  [left: unknown, operator: string, right: unknown, dialect: string],
  WireQuery
> &
  Method<"isNull" | "isNotNull", [value: unknown], WireQuery> &
  Method<
    "inArray" | "notInArray",
    [value: unknown, items: unknown[]],
    WireQuery
  > &
  Method<
    "between" | "notBetween",
    [value: unknown, lower: unknown, upper: unknown],
    WireQuery
  > &
  Method<
    "likeSql" | "notLikeSql",
    [value: unknown, pattern: unknown],
    WireQuery
  > &
  Method<"existsSql" | "notExistsSql", [query: WireQuery], WireQuery> &
  Method<"and" | "or", [conditions: unknown[]], WireQuery> &
  Method<
    "fnCall",
    [name: string, args: unknown[], dialect: string],
    WireQuery
  > &
  Method<"scalarCase", [branches: unknown[], elseVal?: unknown], WireQuery> &
  Method<
    "arithBinary",
    [left: unknown, operator: string, right: unknown, dialect: string],
    WireQuery
  > &
  Method<
    "overClause",
    [
      query: WireQuery,
      partitionBy: unknown[],
      orderBy: unknown[],
      dialect: string,
    ],
    WireQuery
  >;

// ─── Builder sections ────────────────────────────────────────────────────────

type BuilderUtility = Method<"builderNew", [dialect?: string], string> &
  BuilderMethod<"builderClone" | "builderClear"> &
  BuilderMethod<"builderDrop", [], void>;

type BuilderFrom = BuilderMethod<"builderFromTable", [table: string]> &
  BuilderMethod<"builderFromTableAlias", [table: string, alias: string]> &
  BuilderMethod<"builderFromSubquery", [alias: string, queryJson: string]> &
  BuilderMethod<
    "builderFromSubqueryHandle",
    [alias: string, rhsHandle: string]
  >;

type BuilderDistinct = BuilderMethod<"builderDistinct"> &
  BuilderMethod<
    "builderDistinctOnColumns" | "builderDistinctOnExprs",
    [json: string]
  >;

type BuilderSelect = BuilderMethod<
  "builderSelectColumns" | "builderSelectAliased",
  [json: string]
> &
  BuilderMethod<
    "builderSelectFragment",
    [fragmentJson: string, selectedColsJson: string]
  >;

type BuilderJoin = BuilderMethod<
  "builderJoinTable",
  [joinType: string, table: string]
> &
  BuilderMethod<
    "builderJoinTableAlias",
    [joinType: string, table: string, alias: string]
  > &
  BuilderMethod<
    "builderJoinSubquery",
    [joinType: string, alias: string, queryJson: string]
  > &
  BuilderMethod<
    "builderJoinSubqueryHandle",
    [joinType: string, alias: string, rhsHandle: string]
  > &
  BuilderMethod<
    "builderOn" | "builderAndOn" | "builderOrOn",
    [predJson: string]
  > &
  BuilderMethod<"builderUsingColumns", [colsJson: string]> &
  BuilderMethod<"builderOnColumns", [pairsJson: string]>;

type BuilderWhere = BuilderMethod<
  "builderWhere" | "builderAndWhere" | "builderOrWhere",
  [predJson: string]
> &
  BuilderMethod<
    "builderHaving" | "builderAndHaving" | "builderOrHaving",
    [predJson: string]
  >;

type BuilderGroupOrderLimit = BuilderMethod<
  "builderGroupByColumns",
  [colsJson: string]
> &
  BuilderMethod<
    "builderOrderByColumn",
    [col: string, direction?: string, nullOrder?: string]
  > &
  BuilderMethod<"builderOrderByColumns", [colsJson: string]> &
  BuilderMethod<"builderLimit" | "builderOffset", [count: number]> &
  BuilderMethod<
    | "builderForUpdate"
    | "builderForShare"
    | "builderNoWait"
    | "builderSkipLocked"
  >;

type BuilderCompound = BuilderMethod<
  "builderUnion" | "builderUnionAll" | "builderIntersect" | "builderExcept",
  [queryJson: string]
> &
  BuilderMethod<
    | "builderUnionHandle"
    | "builderUnionAllHandle"
    | "builderIntersectHandle"
    | "builderExceptHandle",
    [rhsHandle: string]
  >;

type BuilderCte = BuilderMethod<
  "builderWith" | "builderWithRecursive",
  [name: string, queryJson: string]
> &
  BuilderMethod<
    "builderWithHandle" | "builderWithRecursiveHandle",
    [name: string, rhsHandle: string]
  >;

type BuilderInsert = BuilderMethod<"builderInsertInto", [table: string]> &
  BuilderMethod<"builderColumns", [colsJson: string]> &
  BuilderMethod<"builderValuesInsert", [rowsJson: string]> &
  BuilderMethod<"builderInsertSelect", [queryJson: string]> &
  BuilderMethod<"builderInsertSelectHandle", [rhsHandle: string]> &
  BuilderMethod<"builderOnConflictColumns", [colsJson: string]> &
  BuilderMethod<"builderOnConflictConstraint", [constraint: string]> &
  BuilderMethod<"builderDoNothing"> &
  BuilderMethod<"builderDoUpdateSet", [assignmentsJson: string]> &
  BuilderMethod<"builderConflictWhere", [predJson: string]> &
  BuilderMethod<
    "builderReturningColumns" | "builderReturningAliased",
    [json: string]
  > &
  BuilderMethod<
    "builderReturningFragment",
    [fragmentJson: string, selectedColsJson: string]
  >;

type BuilderUpdate = BuilderMethod<"builderUpdate", [table: string]> &
  BuilderMethod<"builderSet", [assignmentsJson: string]>;

type BuilderDelete = BuilderMethod<"builderDeleteFrom", [table: string]>;

type BuilderOutput = BuilderMethod<"builderQuery", [], WireQuery> &
  BuilderMethod<"builderText" | "builderRaw", [], string> &
  BuilderMethod<"builderValues", [], unknown[]> &
  BuilderMethod<
    "builderSelectedColumns" | "builderInsertColumns",
    [],
    string[]
  > &
  BuilderMethod<"builderAs", [alias: string], WireAliasedQuery>;

type BuilderBatch = BuilderMethod<"builderApplyOps", [opsJson: string]>;

type OptionalBuilderMethods = OptionalMethod<
  "builderConflictTargetKind",
  [handle: string],
  string
> &
  OptionalMethod<
    "builderCompileBundle",
    [handle: string],
    { text: string; raw: string; values: unknown[] }
  > &
  OptionalMethod<"builderCanonicalIrHash", [handle: string], string> &
  OptionalMethod<
    "builderApplyOpsBinary",
    [handle: string, payload: Uint8Array],
    string
  >;

// ─── Runtime binding ─────────────────────────────────────────────────────────

export type RuntimeBinding = SqlPrimitives &
  Expressions &
  BuilderUtility &
  BuilderFrom &
  BuilderDistinct &
  BuilderSelect &
  BuilderJoin &
  BuilderWhere &
  BuilderGroupOrderLimit &
  BuilderCompound &
  BuilderCte &
  BuilderInsert &
  BuilderUpdate &
  BuilderDelete &
  BuilderOutput &
  BuilderBatch &
  OptionalBuilderMethods;
