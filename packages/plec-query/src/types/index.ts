import type { Merge, Simplify } from "type-fest";
import type { z } from "zod";
import type { DIALECTS } from "#core";
import type {
  SqlIdentifierSchema,
  SqlQuerySchema,
  SqlRawSchema,
} from "#schemas";
import type { buildPredicateForOp } from "../main/builder/build-predicate-for-operation";
/**
 * Maps dataset variable names to string values.
 *
 * @remarks Used for named substitution values.
 *
 * @example
 * ```ts
 * const vars: DatasetVariables = { limit: "10" };
 * ```
 */
export type DatasetVariables = Record<string, string>;
/**
 * Infers the normalized SQL query payload shape.
 *
 * @remarks Matches the runtime contract produced by the query builder.
 *
 * @example
 * ```ts
 * const query: SqlQuery = { text: "select 1", raw: "select 1", values: [] };
 * ```
 */
export type SqlQuery = z.infer<typeof SqlQuerySchema>;
/**
 * Infers the SQL identifier wrapper shape.
 *
 * @remarks Represents a value that should render as an identifier.
 *
 * @example
 * ```ts
 * const identifier: SqlIdentifier = { kind: "identifier", names: ["users", "id"] } as SqlIdentifier;
 * ```
 */
export type SqlIdentifier = z.infer<typeof SqlIdentifierSchema>;
/**
 * Infers the raw SQL fragment wrapper shape.
 *
 * @remarks Represents SQL text that should bypass normal escaping.
 *
 * @example
 * ```ts
 * const raw: SqlRaw = { kind: "raw", text: "now()" } as SqlRaw;
 * ```
 */
export type SqlRaw = z.infer<typeof SqlRawSchema>;
/**
 * Describes a connection that can execute a {@link SqlQuery}.
 *
 * @remarks The result type tracks the caller's execution layer.
 *
 * @example
 * ```ts
 * const connection: DatabaseConnection<number> = {
 * 	execute: async () => 1,
 * };
 * ```
 */
export type DatabaseConnection<TResult = unknown> = {
  execute(query: SqlQuery): TResult | Promise<TResult>;
};
/**
 * Specializes {@link DatabaseConnection} to an unknown result.
 *
 * @remarks Useful when the execution layer does not expose result typing.
 *
 * @example
 * ```ts
 * const connection: AnyDatabaseConnection = {
 * 	execute: async () => ({ rows: [] }),
 * };
 * ```
 */
export type AnyDatabaseConnection = DatabaseConnection<unknown>;

/**
 * Represents one parameter value inside a {@link SqlQuery}.
 *
 * @remarks Derived from the query schema's values array.
 *
 * @example
 * ```ts
 * const value: Primitive = 42;
 * ```
 */
export type Primitive = SqlQuery["values"][number];
/**
 * Stores the rendered text and values for a compiled query.
 *
 * @remarks The raw field preserves the pre-dialect rendering.
 *
 * @example
 * ```ts
 * const compiled: CompiledQuery = { text: "select ?", raw: "select ?", values: [1] };
 * ```
 */
export type CompiledQuery = {
  text: string;
  values: Primitive[];
  raw: string;
};
/**
 * Defines the payload sent to a remote Postgres query endpoint.
 *
 * @remarks Remote execution currently fixes the dialect to Postgres.
 *
 * @example
 * ```ts
 * const request: RemoteQueryRequest = { dialect: "postgres", text: "select 1", raw: "select 1", values: [] };
 * ```
 */
export type RemoteQueryRequest = {
  dialect: "postgres";
  text: string;
  values: Primitive[];
  raw: string;
};
/**
 * Accepts any value that can be embedded into SQL.
 *
 * @remarks Includes scalar values, arrays, nested queries, and wrappers.
 *
 * @example
 * ```ts
 * const value: SqlValue = [1, 2, 3];
 * ```
 */
export type SqlValue =
  | Primitive
  | Primitive[]
  | SqlQuery
  | SqlIdentifier
  | SqlRaw;

/**
 * Narrows the supported SQL dialect names.
 *
 * @remarks Derived from the runtime dialect registry.
 *
 * @example
 * ```ts
 * const dialect: Dialect = "postgres";
 * ```
 */
export type Dialect = typeof DIALECTS extends Set<infer T> ? T : never;

/**
 * Names a SQL feature that may be unavailable for a dialect.
 *
 * @remarks Used by validation results for unsupported capabilities.
 *
 * @example
 * ```ts
 * const feature: UnsupportedFeature = "RightJoin";
 * ```
 */
export type UnsupportedFeature =
  | "RightJoin"
  | "FullJoin"
  | "ReturningOnUpdate"
  | "WindowFunction";

/**
 * Describes one unsupported-feature validation detail.
 *
 * @remarks Each entry uses a stable code and a typed feature name.
 *
 * @example
 * ```ts
 * const detail: QueryValidationErrorDetail = {
 * 	code: "right_join",
 * 	message: "RIGHT JOIN is not supported.",
 * 	feature: "RightJoin",
 * };
 * ```
 */
export type QueryValidationErrorDetail = {
  /** Stable snake_case machine code (e.g. `"right_join"`). */
  code: string;
  /** Human-readable explanation. */
  message: string;
  /** Typed discriminant for the unsupported feature. */
  feature: UnsupportedFeature;
};
/**
 * Accepts any plain Zod object schema.
 *
 * @remarks Used where only object-like Zod shapes are valid.
 *
 * @example
 * ```ts
 * type Schema = AnyZodObject;
 * ```
 */
export type AnyZodObject = z.ZodObject<z.ZodRawShape>;
/**
 * Accepts any database schema object.
 *
 * @remarks Table metadata is derived from this Zod object shape.
 *
 * @example
 * ```ts
 * type Schema = AnyDatabaseSchema;
 * ```
 */
export type AnyDatabaseSchema = z.ZodObject<z.ZodRawShape>;
/**
 * Lists the builder stages that control fluent API availability.
 *
 * @remarks Stage names drive conditional type gating across the builder.
 *
 * @example
 * ```ts
 * const stage: QueryStage = "select";
 * ```
 */
export type QueryStage =
  | "start"
  | "cte"
  | "from"
  | "distinct"
  | "select"
  | "insertInto"
  | "insertColumns"
  | "insertValues"
  | "insertSelect"
  | "insertConflictTarget"
  | "insertConflictUpdate"
  | "insertConflictAction"
  | "returning"
  | "update"
  | "set"
  | "delete"
  | "join"
  | "joinPending"
  | "joinPendingReady"
  | "where"
  | "groupBy"
  | "having"
  | "orderBy"
  | "limit"
  | "offset"
  | "compound";

/**
 * Tracks builder capabilities for a specific fluent state.
 *
 * @remarks The boolean flags determine which accessors remain valid.
 *
 * @example
 * ```ts
 * type State = BuilderState<"select", true, true, false, false, false>;
 * ```
 */
export type BuilderState<
  TStage extends QueryStage = QueryStage,
  THasFrom extends boolean = boolean,
  THasSelect extends boolean = boolean,
  THasGroupBy extends boolean = boolean,
  THasLimit extends boolean = boolean,
  THasOffset extends boolean = boolean,
> = {
  stage: TStage;
  hasFrom: THasFrom;
  hasSelect: THasSelect;
  hasGroupBy: THasGroupBy;
  hasLimit: THasLimit;
  hasOffset: THasOffset;
  isCompleteSelectQuery: THasFrom extends true
    ? THasSelect extends true
      ? TStage extends "joinPending"
        ? false
        : true
      : false
    : false;
  isCompleteQuery: TStage extends
    | "insertValues"
    | "insertSelect"
    | "insertConflictTarget"
    | "insertConflictUpdate"
    | "insertConflictAction"
    | "returning"
    | "set"
    | "delete"
    | "where"
    ? true
    : THasFrom extends true
      ? THasSelect extends true
        ? TStage extends "joinPending"
          ? false
          : true
        : false
      : false;
};

/**
 * Represents any {@link BuilderState} specialization.
 *
 * @remarks Useful when a helper accepts builder state generically.
 *
 * @example
 * ```ts
 * type State = AnyBuilderState;
 * ```
 */
export type AnyBuilderState = BuilderState;
/**
 * Represents the untouched builder state.
 *
 * @remarks This is the state before any query clause is applied.
 *
 * @example
 * ```ts
 * type State = InitialBuilderState;
 * ```
 */
export type InitialBuilderState = BuilderState<
  "start",
  false,
  false,
  false,
  false,
  false
>;
/**
 * Maps source aliases to their visible column names.
 *
 * @remarks Used to compute column references across joined sources.
 *
 * @example
 * ```ts
 * const sources: SourceColumnMap = { users: "id" };
 * ```
 */
export type SourceColumnMap = Record<string, string>;
/**
 * Represents any {@link SourceColumnMap} specialization.
 *
 * @remarks Useful when a helper should accept arbitrary source maps.
 *
 * @example
 * ```ts
 * type Sources = AnySourceColumnMap;
 * ```
 */
export type AnySourceColumnMap = SourceColumnMap;

/**
 * Extracts table names from a schema object.
 *
 * @remarks Only array-of-record properties are treated as tables.
 *
 * @example
 * ```ts
 * type Name = TableName<AnyDatabaseSchema>;
 * ```
 */
export type TableName<TSchema extends AnyDatabaseSchema> = Extract<
  {
    [TKey in keyof z.infer<TSchema>]: z.infer<TSchema>[TKey] extends Array<
      Record<string, unknown>
    >
      ? TKey
      : never;
  }[keyof z.infer<TSchema>],
  string
>;

/**
 * Extracts one row shape from a schema table.
 *
 * @remarks The table must resolve to an array of row objects.
 *
 * @example
 * ```ts
 * type Row = TableRow<AnyDatabaseSchema, TableName<AnyDatabaseSchema>>;
 * ```
 */
export type TableRow<
  TSchema extends AnyDatabaseSchema,
  TTable extends TableName<TSchema>,
> = z.infer<TSchema>[TTable] extends Array<infer TRow> ? TRow : never;

/**
 * Extracts column names from a schema table row.
 *
 * @remarks Only string keys are kept.
 *
 * @example
 * ```ts
 * type Column = ColumnName<AnyDatabaseSchema, TableName<AnyDatabaseSchema>>;
 * ```
 */
export type ColumnName<
  TSchema extends AnyDatabaseSchema,
  TTable extends TableName<TSchema>,
> = Extract<keyof TableRow<TSchema, TTable>, string>;

/**
 * Builds a qualified column name from an alias and column.
 *
 * @remarks Produces the dotted `alias.column` form.
 *
 * @example
 * ```ts
 * type Name = QualifiedColumnName<"users", "id">;
 * ```
 */
export type QualifiedColumnName<
  TAlias extends string,
  TColumn extends string,
> = `${TAlias}.${TColumn}`;

/**
 * Collects all qualified column names from a source map.
 *
 * @remarks Each entry is generated as `alias.column`.
 *
 * @example
 * ```ts
 * type Names = QualifiedColumnNames<{ users: "id" | "email" }>;
 * ```
 */
export type QualifiedColumnNames<TSources extends AnySourceColumnMap> = Extract<
  {
    [TAlias in keyof TSources & string]: QualifiedColumnName<
      TAlias,
      Extract<TSources[TAlias], string>
    >;
  }[keyof TSources & string],
  string
>;

/**
 * Extracts the last segment from a dotted identifier.
 *
 * @remarks Undotted values pass through unchanged.
 *
 * @example
 * ```ts
 * type Column = LastSegment<"users.id">;
 * ```
 */
export type LastSegment<TValue extends string> =
  TValue extends `${string}.${infer TColumn}` ? TColumn : TValue;

/**
 * Accepts either default columns or qualified source columns.
 *
 * @remarks This is the base type for column references in expressions.
 *
 * @example
 * ```ts
 * type Ref = ColumnReference<{ users: "id" }, "name">;
 * ```
 */
export type ColumnReference<
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
> = Extract<TDefaultColumns | QualifiedColumnNames<TSources>, string>;

/**
 * Merges two source maps with right-hand precedence.
 *
 * @remarks Used when joins introduce new aliases over existing ones.
 *
 * @example
 * ```ts
 * type Sources = MergeSources<{ users: "id" }, { posts: "id" }>;
 * ```
 */
export type MergeSources<
  TLeft extends AnySourceColumnMap,
  TRight extends AnySourceColumnMap,
> = Simplify<Merge<TLeft, TRight>>;

/**
 * Stores a select query together with its alias metadata.
 *
 * @remarks Used when a completed query becomes a derived table.
 *
 * @example
 * ```ts
 * type Query = AliasedQuery<"recent_users", "id">;
 * ```
 */
export type AliasedQuery<TAlias extends string, TColumns extends string> = {
  readonly __kind: "aliased-query";
  readonly alias: TAlias;
  readonly query: SqlQuery;
  readonly text: string;
  readonly raw: string;
  readonly values: Primitive[];
  readonly __handle?: string;
  readonly selectedColumns?: string[];
  readonly __columns?: TColumns;
};

/**
 * Represents an {@link AliasedQuery} with broad string metadata.
 *
 * @remarks Useful for helpers that do not preserve alias literals.
 *
 * @example
 * ```ts
 * type Query = AnyAliasedQuery;
 * ```
 */
export type AnyAliasedQuery = AliasedQuery<string, string>;
/**
 * Stores a query operand together with optional selected-column metadata.
 *
 * @remarks Used by set operations and subquery plumbing.
 *
 * @example
 * ```ts
 * type Operand = QueryOperand<"id">;
 * ```
 */
export type QueryOperand<TColumns extends string = string> = {
  readonly query: SqlQuery;
  readonly selectedColumns?: string[];
  readonly __columns?: TColumns;
};
/**
 * Represents a {@link QueryOperand} with widened column metadata.
 *
 * @remarks Useful when exact selected columns are not known.
 *
 * @example
 * ```ts
 * type Operand = AnyQueryOperand;
 * ```
 */
export type AnyQueryOperand = QueryOperand<string>;

/**
 * Accepts the supported ordering direction spellings.
 *
 * @remarks Both upper-case and lower-case forms are allowed.
 *
 * @example
 * ```ts
 * const direction: OrderDirection = "DESC";
 * ```
 */
export type OrderDirection = "ASC" | "DESC" | "asc" | "desc";
/**
 * Accepts the supported null ordering spellings.
 *
 * @remarks Both upper-case and lower-case forms are allowed.
 *
 * @example
 * ```ts
 * const nulls: NullOrder = "LAST";
 * ```
 */
export type NullOrder = "FIRST" | "LAST" | "first" | "last";
/**
 * Accepts comparison operators for predicate helpers.
 *
 * @remarks Used by builder context comparison methods.
 *
 * @example
 * ```ts
 * const operator: ComparisonOperator = ">=";
 * ```
 */
export type ComparisonOperator = "=" | "!=" | "<>" | ">" | ">=" | "<" | "<=";

/**
 * Configures a database instance's dialect, connection, and schema.
 *
 * @remarks Each field is optional so callers can opt into typing gradually.
 *
 * @example
 * ```ts
 * const config: DatabaseConfig = { dialect: "postgres" };
 * ```
 */
export type DatabaseConfig<
  TSchema extends AnyDatabaseSchema | undefined = undefined,
  TConnection extends AnyDatabaseConnection | undefined = undefined,
> = {
  dialect?: Dialect;
  connection?: TConnection;
  schema?: TSchema;
};

/**
 * Accepts a predicate string or a prebuilt {@link SqlQuery}.
 *
 * @remarks Used by where, having, and join predicate helpers.
 *
 * @example
 * ```ts
 * const predicate: PredicateInput = "users.id = posts.user_id";
 * ```
 */
export type PredicateInput = SqlQuery | string;
/**
 * Names the supported SQL join keywords.
 *
 * @remarks Used by join builder operations and validation.
 *
 * @example
 * ```ts
 * const join: JoinType = "LEFT JOIN";
 * ```
 */
export type JoinType = "INNER JOIN" | "LEFT JOIN" | "RIGHT JOIN" | "FULL JOIN";
/**
 * Names the boolean operators used between clauses.
 *
 * @remarks Used for chained predicate composition.
 *
 * @example
 * ```ts
 * const operator: ClauseOperator = "AND";
 * ```
 */
export type ClauseOperator = "AND" | "OR";
/**
 * Names the supported compound-query operators.
 *
 * @remarks Used by union, intersect, and except helpers.
 *
 * @example
 * ```ts
 * const operator: CompoundOperator = "UNION ALL";
 * ```
 */
export type CompoundOperator = "UNION" | "UNION ALL" | "INTERSECT" | "EXCEPT";

/**
 * Narrows selectable column names for the current source set.
 *
 * @remarks This is the base column type for select inputs.
 *
 * @example
 * ```ts
 * type Column = SelectedColumn<{ users: "id" }, never>;
 * ```
 */
export type SelectedColumn<
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
> = ColumnReference<TSources, TDefaultColumns>;

/**
 * Maps output property names to selected columns.
 *
 * @remarks Used by object-form select calls.
 *
 * @example
 * ```ts
 * type Columns = SelectedColumnRecord<{ users: "id" }, never>;
 * ```
 */
export type SelectedColumnRecord<
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
> = Record<string, SelectedColumn<TSources, TDefaultColumns>>;

/**
 * Describes one ORDER BY item inside a window specification.
 *
 * @remarks The expression is always stored as a {@link SqlQuery}.
 *
 * @example
 * ```ts
 * const item: WindowOrderItem = { expression: { text: "id", raw: "id", values: [] } as SqlQuery };
 * ```
 */
export type WindowOrderItem = {
  expression: SqlQuery;
  direction?: "ASC" | "DESC";
  nulls?: "FIRST" | "LAST";
};

/**
 * Describes a full window clause.
 *
 * @remarks Partition and order expressions are tracked separately.
 *
 * @example
 * ```ts
 * const spec: WindowSpec = { partitionBy: [], orderBy: [] };
 * ```
 */
export type WindowSpec = {
  partitionBy: SqlQuery[];
  orderBy: WindowOrderItem[];
};

/**
 * Represents a function expression that can be aliased or windowed.
 *
 * @remarks Extends {@link SqlQuery} with builder-facing metadata helpers.
 *
 * @example
 * ```ts
 * const fnExpr = {} as FunctionExpression;
 * fnExpr.as("total");
 * ```
 */
export type FunctionExpression = SqlQuery & {
  readonly __kind: "function-expression";
  readonly __exprNode?: SqlQuery;
  readonly overClause?: WindowSpec;
  as<TAlias extends string>(alias: TAlias): AliasedSelectExpression<TAlias>;
  over(build?: (builder: WindowBuilder) => WindowBuilder): FunctionExpression;
};

/**
 * Stores a projected expression together with its selected alias.
 *
 * @remarks Used by array-based select lists and returning clauses.
 *
 * @example
 * ```ts
 * type Expr = AliasedSelectExpression<"total">;
 * ```
 */
export type AliasedSelectExpression<TAlias extends string = string> = {
  readonly __kind: "aliased-select-expression";
  readonly alias: TAlias;
  readonly query: SqlQuery;
  readonly text: string;
  readonly raw: string;
  readonly values: Primitive[];
};

/**
 * Provides fluent helpers for building window clauses.
 *
 * @remarks Each method returns the same window builder shape.
 *
 * @example
 * ```ts
 * const builder = {} as WindowBuilder;
 * builder.partitionBy("users.id");
 * ```
 */
export type WindowBuilder = {
  partitionBy(
    ...expressions: Array<
      | string
      | NonStringPrimitive
      | ValueLiteral<Primitive>
      | SqlQuery
      | SqlIdentifier
    >
  ): WindowBuilder;
  orderBy(
    expression:
      | string
      | NonStringPrimitive
      | ValueLiteral<Primitive>
      | SqlQuery
      | SqlIdentifier,
    direction?: OrderDirection,
    nulls?: NullOrder,
  ): WindowBuilder;
};

/**
 * Accepts one selectable expression value.
 *
 * @remarks Select expressions may be columns, raw queries, or functions.
 *
 * @example
 * ```ts
 * type Value = SelectExpressionValue<{ users: "id" }, never>;
 * ```
 */
export type SelectExpressionValue<
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
> = SelectedColumn<TSources, TDefaultColumns> | SqlQuery | FunctionExpression;

/**
 * Maps output property names to selectable expressions.
 *
 * @remarks Used by object-form projection helpers.
 *
 * @example
 * ```ts
 * type RecordShape = SelectExpressionRecord<{ users: "id" }, never>;
 * ```
 */
export type SelectExpressionRecord<
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
> = Record<string, SelectExpressionValue<TSources, TDefaultColumns>>;

/**
 * Accepts one item in an array-form select list.
 *
 * @remarks Array selections may include plain columns or aliased expressions.
 *
 * @example
 * ```ts
 * type Value = SelectArrayValue<{ users: "id" }, never>;
 * ```
 */
export type SelectArrayValue<
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
> = SelectedColumn<TSources, TDefaultColumns> | AliasedSelectExpression<string>;

/**
 * Accepts every supported select input shape.
 *
 * @remarks Select calls may use arrays, plain column maps, or expression maps.
 *
 * @example
 * ```ts
 * type Input = SelectInput<{ users: "id" }, never>;
 * ```
 */
export type SelectInput<
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
> =
  | Array<SelectArrayValue<TSources, TDefaultColumns>>
  | SelectedColumnRecord<TSources, TDefaultColumns>
  | SelectExpressionRecord<TSources, TDefaultColumns>;

/**
 * Accepts one DISTINCT ON expression.
 *
 * @remarks Distinct-on inputs share the select-expression value rules.
 *
 * @example
 * ```ts
 * type Value = DistinctOnValue<{ users: "id" }, never>;
 * ```
 */
export type DistinctOnValue<
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
> = SelectExpressionInput<SelectedColumn<TSources, TDefaultColumns>>;

/**
 * Accepts all DISTINCT ON expressions for a query.
 *
 * @remarks The expressions are evaluated in array order.
 *
 * @example
 * ```ts
 * type Input = DistinctOnInput<{ users: "id" }, never>;
 * ```
 */
export type DistinctOnInput<
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
> = Array<DistinctOnValue<TSources, TDefaultColumns>>;

/**
 * Derives output column names from a select input shape.
 *
 * @remarks Aliased expressions contribute their alias names.
 *
 * @example
 * ```ts
 * type Columns = SelectedOutputColumns<["users.id"]>;
 * ```
 */
export type SelectedOutputColumns<TColumns> =
  TColumns extends Array<infer TColumn>
    ? TColumn extends string
      ? LastSegment<TColumn>
      : TColumn extends AliasedSelectExpression<infer TAlias>
        ? TAlias
        : never
    : TColumns extends Record<string, unknown>
      ? Extract<keyof TColumns, string>
      : never;

/**
 * Accepts a column that may be used in ORDER BY.
 *
 * @remarks Ordering can target visible source columns or selected aliases.
 *
 * @example
 * ```ts
 * type Column = OrderableColumn<{ users: "id" }, never, "total">;
 * ```
 */
export type OrderableColumn<
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
> = Extract<
  SelectedColumn<TSources, TDefaultColumns> | TSelectedColumns,
  string
>;

/**
 * Extracts the visible column union from a source map.
 *
 * @remarks Used when only source column names are needed.
 *
 * @example
 * ```ts
 * type Name = SourceColumnName<{ users: "id" | "email" }>;
 * ```
 */
export type SourceColumnName<TSources extends AnySourceColumnMap> = Extract<
  TSources[keyof TSources & string],
  string
>;

/**
 * Produces a readable compile-time error marker.
 *
 * @remarks The message literal is surfaced through the type system.
 *
 * @example
 * ```ts
 * type Error = BuilderError<"Missing select">;
 * ```
 */
export type BuilderError<TMessage extends string> = {
  readonly __tsQueryError: TMessage;
};

/**
 * Produces an error type for early query accessor reads.
 *
 * @remarks Used when callers access query properties before completion.
 *
 * @example
 * ```ts
 * type Error = IncompleteQueryAccess<"query">;
 * ```
 */
export type IncompleteQueryAccess<TAccessor extends string> =
  BuilderError<`Cannot access ${TAccessor} before completing the query`>;

type IncompleteAliasAccess =
  BuilderError<"Cannot alias a query before calling from(...).select(...)">;

/**
 * Exposes a value only when the select query is complete.
 *
 * @remarks Otherwise a typed compile-time error is returned instead.
 *
 * @example
 * ```ts
 * type Access = QueryAccess<InitialBuilderState, SqlQuery, "query">;
 * ```
 */
export type QueryAccess<
  TState extends AnyBuilderState,
  TValue,
  TAccessor extends string,
> = TState["isCompleteSelectQuery"] extends true
  ? TValue
  : IncompleteQueryAccess<TAccessor>;

/**
 * Exposes a value only when any complete query state is reached.
 *
 * @remarks DML completion also satisfies this access check.
 *
 * @example
 * ```ts
 * type Access = CompleteQueryAccess<InitialBuilderState, SqlQuery, "query">;
 * ```
 */
export type CompleteQueryAccess<
  TState extends AnyBuilderState,
  TValue,
  TAccessor extends string,
> = TState["isCompleteQuery"] extends true
  ? TValue
  : IncompleteQueryAccess<TAccessor>;

/**
 * Exposes a value only when aliasing is valid.
 *
 * @remarks Incomplete select queries resolve to a typed alias error.
 *
 * @example
 * ```ts
 * type Access = AliasAccess<InitialBuilderState, SqlQuery>;
 * ```
 */
export type AliasAccess<
  TState extends AnyBuilderState,
  TValue,
> = TState["isCompleteSelectQuery"] extends true
  ? TValue
  : IncompleteAliasAccess;

/**
 * Removes string values from {@link Primitive}.
 *
 * @remarks Useful for APIs that distinguish identifiers from scalar strings.
 *
 * @example
 * ```ts
 * const value: NonStringPrimitive = 1;
 * ```
 */
export type NonStringPrimitive = Exclude<Primitive, string>;
/**
 * Wraps a primitive as an explicit value literal.
 *
 * @remarks Used to disambiguate literal values from column references.
 *
 * @example
 * ```ts
 * const value: ValueLiteral<number> = { __kind: "value", value: 1 };
 * ```
 */
export type ValueLiteral<TValue extends Primitive> = {
  readonly __kind: "value";
  readonly value: TValue;
};

/**
 * Accepts a value that may be written into INSERT or UPDATE clauses.
 *
 * @remarks Write values support literals, nested queries, and identifiers.
 *
 * @example
 * ```ts
 * const value: WriteValue = { __kind: "value", value: 1 };
 * ```
 */
export type WriteValue =
  | Primitive
  | ValueLiteral<Primitive>
  | SqlQuery
  | SqlIdentifier;

/**
 * Maps insert column names to optional write values.
 *
 * @remarks Partial rows let callers omit columns handled by defaults.
 *
 * @example
 * ```ts
 * type Row = InsertRow<"id" | "name">;
 * ```
 */
export type InsertRow<TColumns extends string> = Partial<
  Record<TColumns, WriteValue>
>;

/**
 * Maps update column names to optional write values.
 *
 * @remarks Partial assignments let callers update only changed columns.
 *
 * @example
 * ```ts
 * type Set = UpdateSet<"name">;
 * ```
 */
export type UpdateSet<TColumns extends string> = Partial<
  Record<TColumns, WriteValue>
>;

/**
 * Accepts one expression value available to select-style helpers.
 *
 * @remarks Inputs may be column names, literals, queries, or identifiers.
 *
 * @example
 * ```ts
 * type Input = SelectExpressionInput<"users.id">;
 * ```
 */
export type SelectExpressionInput<TAvailable extends string> =
  | TAvailable
  | NonStringPrimitive
  | ValueLiteral<Primitive>
  | SqlQuery
  | SqlIdentifier;

/**
 * Describes one WHEN ... THEN branch in a CASE expression.
 *
 * @remarks The condition and result share the builder's available columns.
 *
 * @example
 * ```ts
 * type Branch = CaseBranch<"users.id">;
 * ```
 */
export type CaseBranch<TAvailable extends string> = {
  when: PredicateInput;
  then: SelectExpressionInput<TAvailable>;
};

/**
 * Provides typed helper methods inside builder callbacks.
 *
 * @remarks The available column union scopes every expression helper.
 *
 * @example
 * ```ts
 * const ctx = {} as BuilderContext<"users.id">;
 * ctx.col("users.id");
 * ```
 */
export type BuilderContext<TAvailable extends string> = {
  col<TColumn extends TAvailable>(column: TColumn): TColumn;
  ref<TColumn extends TAvailable>(column: TColumn): SqlIdentifier;
  excluded<TColumn extends string>(column: TColumn): SqlQuery;
  val<TValue extends Primitive>(value: TValue): ValueLiteral<TValue>;
  fn(
    name: string,
    ...args: Array<SelectExpressionInput<TAvailable> | "*" | SqlIdentifier>
  ): FunctionExpression;
  agg(
    name: string,
    ...args: Array<SelectExpressionInput<TAvailable> | "*" | SqlIdentifier>
  ): FunctionExpression;
  lag(
    value: TAvailable | SqlQuery | SqlIdentifier,
    offset?: NonStringPrimitive | ValueLiteral<Primitive>,
    defaultValue?: SelectExpressionInput<TAvailable>,
  ): FunctionExpression;
  lead(
    value: TAvailable | SqlQuery | SqlIdentifier,
    offset?: NonStringPrimitive | ValueLiteral<Primitive>,
    defaultValue?: SelectExpressionInput<TAvailable>,
  ): FunctionExpression;
  rowNumber(): FunctionExpression;
  rank(): FunctionExpression;
  denseRank(): FunctionExpression;
  cmp<TLeft extends TAvailable>(
    left: TLeft,
    operator: ComparisonOperator,
    right: TAvailable | NonStringPrimitive | ValueLiteral<Primitive>,
  ): SqlQuery;
  eq<TLeft extends TAvailable>(
    left: TLeft,
    right: TAvailable | NonStringPrimitive | ValueLiteral<Primitive>,
  ): SqlQuery;
  ne<TLeft extends TAvailable>(
    left: TLeft,
    right: TAvailable | NonStringPrimitive | ValueLiteral<Primitive>,
  ): SqlQuery;
  gt<TLeft extends TAvailable>(
    left: TLeft,
    right: TAvailable | NonStringPrimitive | ValueLiteral<Primitive>,
  ): SqlQuery;
  gte<TLeft extends TAvailable>(
    left: TLeft,
    right: TAvailable | NonStringPrimitive | ValueLiteral<Primitive>,
  ): SqlQuery;
  lt<TLeft extends TAvailable>(
    left: TLeft,
    right: TAvailable | NonStringPrimitive | ValueLiteral<Primitive>,
  ): SqlQuery;
  lte<TLeft extends TAvailable>(
    left: TLeft,
    right: TAvailable | NonStringPrimitive | ValueLiteral<Primitive>,
  ): SqlQuery;
  isNull<TValue extends TAvailable>(value: TValue): SqlQuery;
  isNotNull<TValue extends TAvailable>(value: TValue): SqlQuery;
  inArray<TValue extends TAvailable>(
    value: TValue,
    items: Primitive[] | SqlQuery,
  ): SqlQuery;
  notInArray<TValue extends TAvailable>(
    value: TValue,
    items: Primitive[] | SqlQuery,
  ): SqlQuery;
  between<TValue extends TAvailable>(
    value: TValue,
    lower: TAvailable | NonStringPrimitive | ValueLiteral<Primitive>,
    upper: TAvailable | NonStringPrimitive | ValueLiteral<Primitive>,
  ): SqlQuery;
  notBetween<TValue extends TAvailable>(
    value: TValue,
    lower: TAvailable | NonStringPrimitive | ValueLiteral<Primitive>,
    upper: TAvailable | NonStringPrimitive | ValueLiteral<Primitive>,
  ): SqlQuery;
  like<TValue extends TAvailable>(
    value: TValue,
    pattern: SelectExpressionInput<TAvailable>,
  ): SqlQuery;
  notLike<TValue extends TAvailable>(
    value: TValue,
    pattern: SelectExpressionInput<TAvailable>,
  ): SqlQuery;
  exists(query: SqlQuery): SqlQuery;
  notExists(query: SqlQuery): SqlQuery;
  and(...conditions: PredicateInput[]): SqlQuery;
  or(...conditions: PredicateInput[]): SqlQuery;
  count(
    value?: TAvailable | "*" | SqlQuery | SqlIdentifier,
  ): FunctionExpression;
  sum(value: TAvailable | SqlQuery | SqlIdentifier): FunctionExpression;
  avg(value: TAvailable | SqlQuery | SqlIdentifier): FunctionExpression;
  min(value: TAvailable | SqlQuery | SqlIdentifier): FunctionExpression;
  max(value: TAvailable | SqlQuery | SqlIdentifier): FunctionExpression;
  coalesce(
    ...values: [
      SelectExpressionInput<TAvailable>,
      ...SelectExpressionInput<TAvailable>[],
    ]
  ): FunctionExpression;
  case(
    branches: [CaseBranch<TAvailable>, ...CaseBranch<TAvailable>[]],
    elseValue?: SelectExpressionInput<TAvailable>,
  ): SqlQuery;
  add(
    left: SelectExpressionInput<TAvailable>,
    right: SelectExpressionInput<TAvailable>,
  ): SqlQuery;
  sub(
    left: SelectExpressionInput<TAvailable>,
    right: SelectExpressionInput<TAvailable>,
  ): SqlQuery;
  mul(
    left: SelectExpressionInput<TAvailable>,
    right: SelectExpressionInput<TAvailable>,
  ): SqlQuery;
  div(
    left: SelectExpressionInput<TAvailable>,
    right: SelectExpressionInput<TAvailable>,
  ): SqlQuery;
};

/**
 * Maps one source name to its visible column union.
 *
 * @remarks Used when a helper introduces a new named source.
 *
 * @example
 * ```ts
 * type Source = NamedSourceMap<"users", "id" | "email">;
 * ```
 */
export type NamedSourceMap<TName extends string, TColumns extends string> = {
  [K in TName]: TColumns;
};

/**
 * Accepts every source name available from schema and registered aliases.
 *
 * @remarks Registered sources are merged with schema-backed tables.
 *
 * @example
 * ```ts
 * type Name = AvailableSourceName<AnyDatabaseSchema | undefined, { recent_users: "id" }>;
 * ```
 */
export type AvailableSourceName<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
> = Extract<
  | (TSchema extends AnyDatabaseSchema ? TableName<TSchema> : never)
  | keyof TRegisteredSources,
  string
>;

/**
 * Resolves the visible columns for one available source.
 *
 * @remarks Registered source metadata wins before schema table inference.
 *
 * @example
 * ```ts
 * type Columns = AvailableSourceColumns<AnyDatabaseSchema | undefined, { recent_users: "id" }, "recent_users">;
 * ```
 */
export type AvailableSourceColumns<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSource extends string,
> = TSource extends keyof TRegisteredSources
  ? Extract<TRegisteredSources[TSource], string>
  : TSchema extends AnyDatabaseSchema
    ? TSource extends TableName<TSchema>
      ? ColumnName<TSchema, TSource>
      : never
    : never;

/**
 * Creates a source map entry for an available source alias.
 *
 * @remarks Used after FROM and JOIN calls register a new alias.
 *
 * @example
 * ```ts
 * type Source = AvailableSourceMap<AnyDatabaseSchema | undefined, { recent_users: "id" }, "recent_users", "ru">;
 * ```
 */
export type AvailableSourceMap<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSource extends string,
  TAlias extends string,
> = {
  [K in TAlias]: AvailableSourceColumns<TSchema, TRegisteredSources, TSource>;
};

/**
 * Reuses {@link DatabaseConfig} as a named parameter bundle.
 *
 * @remarks This keeps generic parameter lists consistent across helpers.
 *
 * @example
 * ```ts
 * type Params = DatabaseConfigParams<AnyDatabaseSchema | undefined, AnyDatabaseConnection | undefined>;
 * ```
 */
export type DatabaseConfigParams<
  TSchema extends AnyDatabaseSchema | undefined,
  TConnection extends AnyDatabaseConnection | undefined,
> = DatabaseConfig<TSchema, TConnection>;

/**
 * Produces the database type returned from config construction.
 *
 * @remarks The returned builder starts with no sources and the initial state.
 *
 * @example
 * ```ts
 * type Result = DatabaseConfigReturnParams<AnyDatabaseSchema | undefined, AnyDatabaseConnection | undefined>;
 * ```
 */
export type DatabaseConfigReturnParams<
  TSchema extends AnyDatabaseSchema | undefined,
  TConnection extends AnyDatabaseConnection | undefined,
> = DatabaseType<
  TSchema,
  EmptySourceColumnMap,
  EmptySourceColumnMap,
  never,
  never,
  InitialBuilderState,
  TConnection
>;

/**
 * Stores the runtime builder handle and any retired handles.
 *
 * @remarks Retired handles track ownership changes after cloning.
 *
 * @example
 * ```ts
 * const cell: BuilderHandleCell = { handle: "h1" };
 * ```
 */
export type BuilderHandleCell = {
  handle: string;
  retired?: string[];
};

/**
 * Holds schema and connection generics outside the builder surface.
 *
 * @remarks Used where only those two generic slots need to persist.
 *
 * @example
 * ```ts
 * type Generics = EphemeralGenerics;
 * ```
 */
export type EphemeralGenerics = {
  schema: AnyDatabaseSchema | undefined;
  connection: AnyDatabaseConnection | undefined;
};

/**
 * Bundles the generic slots carried by {@link DatabaseType}.
 *
 * @remarks The phantom object keeps complex generic state accessible to inference.
 *
 * @example
 * ```ts
 * type Params = DatabaseTypeParams<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, string, string, AnyBuilderState, AnyDatabaseConnection | undefined>;
 * ```
 */
export type DatabaseTypeParams<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined,
> = {
  schema: TSchema;
  registeredSources: TRegisteredSources;
  sources: TSources;
  defaultColumns: TDefaultColumns;
  selectedColumns: TSelectedColumns;
  state: TState;
  connection: TConnection;
};

/** Represents any {@link DatabaseTypeParams} specialization. */
export type DbParams = DatabaseTypeParams<
  AnyDatabaseSchema | undefined,
  AnySourceColumnMap,
  AnySourceColumnMap,
  string,
  string,
  AnyBuilderState,
  AnyDatabaseConnection | undefined
>;

/**
 * Represents the public phantom type carried through the builder API.
 *
 * @remarks Runtime instances expose methods while this type tracks compile-time state.
 *
 * @example
 * ```ts
 * type Db = DatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, string, string, AnyBuilderState, AnyDatabaseConnection | undefined>;
 * ```
 */
export type DatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined,
> = {
  readonly __databaseType?: DatabaseTypeParams<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    TState,
    TConnection
  >;
  readonly __columns?: TSelectedColumns;
};

/** Rebuilds a {@link DatabaseType} from its bundled generic parameters. */
export type DbOf<P extends DbParams> = DatabaseType<
  P["schema"],
  P["registeredSources"],
  P["sources"],
  P["defaultColumns"],
  P["selectedColumns"],
  P["state"],
  P["connection"]
>;

/** Exposes a database type only when a type-level condition is satisfied. */
export type Gate<
  P extends DbParams,
  Condition extends boolean,
> = Condition extends true ? DbOf<P> : never;

/** Checks whether a builder state is currently in one of the provided stages. */
export type HasStage<
  TState extends AnyBuilderState,
  TStages extends QueryStage,
> = TState["stage"] extends TStages ? true : false;

/** Exposes a database type only when its state stage is allowed. */
export type StageGate<P extends DbParams, TStages extends QueryStage> = Gate<
  P,
  HasStage<P["state"], TStages>
>;

type StatementStartStage = "start" | "cte";
type SelectableStage = "from" | "distinct" | "join" | "joinPendingReady";
type JoinableStage =
  | "from"
  | "distinct"
  | "select"
  | "join"
  | "joinPendingReady";
type JoinPredicateStage = "joinPending" | "joinPendingReady";
type PostSelectStage =
  | "select"
  | "join"
  | "joinPendingReady"
  | "where"
  | "groupBy"
  | "having"
  | "orderBy";
type CanOrderByStage = PostSelectStage;
type CanLimitStage = PostSelectStage | "offset";
type CanOffsetStage = PostSelectStage | "limit";
type CanSelectLockStage = PostSelectStage | "limit" | "offset";
type CanInsertColumnStage = "insertInto" | "insertColumns";
type CanInsertValuesStage = CanInsertColumnStage;
type CanInsertConflictTargetStage = "insertValues" | "insertSelect";
type CanInsertConflictActionStage =
  | CanInsertConflictTargetStage
  | "insertConflictTarget";
type CanReturnDirectStage =
  | "insertValues"
  | "insertSelect"
  | "insertConflictUpdate"
  | "set"
  | "delete";
type CanSelectWhereStage = "select" | "join" | "joinPendingReady" | "where";
type CanDmlWhereStage = "set" | "delete" | "where";
type CanGroupByStage =
  | "select"
  | "join"
  | "joinPendingReady"
  | "where"
  | "groupBy";
type CanHavingStage = "groupBy" | "having";

/**
 * Exposes the builder type only before a FROM clause has been applied.
 *
 * @remarks Used to gate calls that can only occur before selecting a source.
 *
 * @example
 * ```ts
 * type Db = FromCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, never, never, InitialBuilderState>;
 * ```
 */
export type FromCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = TState["hasFrom"] extends false
  ? DbOf<
      DatabaseTypeParams<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >
    >
  : never;

/**
 * Exposes the builder type only before a CTE source has been fixed.
 *
 * @remarks Shares the same pre-FROM gate as {@link FromCallableDatabaseType}.
 *
 * @example
 * ```ts
 * type Db = CteCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, never, never, InitialBuilderState>;
 * ```
 */
export type CteCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = TState["hasFrom"] extends false
  ? DbOf<
      DatabaseTypeParams<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >
    >
  : never;

/**
 * Exposes the builder type only when DISTINCT can still be applied.
 *
 * @remarks Requires a source but no completed select list.
 *
 * @example
 * ```ts
 * type Db = DistinctCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, string, never, WithFrom<InitialBuilderState>>;
 * ```
 */
export type DistinctCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = TState["hasFrom"] extends true
  ? DbOf<
      DatabaseTypeParams<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >
    >
  : never;

/**
 * Exposes the builder type only when SELECT can be called.
 *
 * @remarks The state must have a source and no existing select list.
 *
 * @example
 * ```ts
 * type Db = SelectableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, string, never, WithFrom<InitialBuilderState>>;
 * ```
 */
export type SelectableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = TState["hasFrom"] extends true
  ? TState["hasSelect"] extends false
    ? HasStage<TState, SelectableStage> extends true
      ? DbOf<
          DatabaseTypeParams<
            TSchema,
            TRegisteredSources,
            TSources,
            TDefaultColumns,
            TSelectedColumns,
            TState,
            TConnection
          >
        >
      : never
    : never
  : never;

/**
 * Exposes the builder type only when INSERT INTO can be called.
 *
 * @remarks Insert statements can start from the initial or CTE stage.
 *
 * @example
 * ```ts
 * type Db = InsertIntoCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, never, never, InitialBuilderState>;
 * ```
 */
export type InsertIntoCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = StageGate<
  DatabaseTypeParams<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    TState,
    TConnection
  >,
  StatementStartStage
>;

/**
 * Exposes the builder type only when INSERT columns can be added.
 *
 * @remarks The state must already target an INSERT statement.
 *
 * @example
 * ```ts
 * type Db = InsertColumnCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, never, never, WithInsertInto>;
 * ```
 */
export type InsertColumnCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = StageGate<
  DatabaseTypeParams<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    TState,
    TConnection
  >,
  CanInsertColumnStage
>;

/**
 * Exposes the builder type only when INSERT values can be supplied.
 *
 * @remarks The INSERT target may already have a narrowed column list.
 *
 * @example
 * ```ts
 * type Db = InsertValuesCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, never, never, WithInsertInto>;
 * ```
 */
export type InsertValuesCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = StageGate<
  DatabaseTypeParams<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    TState,
    TConnection
  >,
  CanInsertValuesStage
>;

/**
 * Exposes the builder type only when a conflict target can be declared.
 *
 * @remarks Conflict targets apply after values or insert-select sources exist.
 *
 * @example
 * ```ts
 * type Db = InsertConflictTargetCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, never, never, WithInsertValues>;
 * ```
 */
export type InsertConflictTargetCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = StageGate<
  DatabaseTypeParams<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    TState,
    TConnection
  >,
  CanInsertConflictTargetStage
>;

/**
 * Exposes the builder type only when a conflict action can be chosen.
 *
 * @remarks The target may already be explicit or still implied.
 *
 * @example
 * ```ts
 * type Db = InsertConflictActionCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, never, never, WithInsertConflictTarget>;
 * ```
 */
export type InsertConflictActionCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = StageGate<
  DatabaseTypeParams<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    TState,
    TConnection
  >,
  CanInsertConflictActionStage
>;

/**
 * Exposes the builder type only when a conflict WHERE clause can be added.
 *
 * @remarks This gate applies only during DO UPDATE conflict handling.
 *
 * @example
 * ```ts
 * type Db = InsertConflictWhereCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, never, never, WithInsertConflictUpdate>;
 * ```
 */
export type InsertConflictWhereCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = StageGate<
  DatabaseTypeParams<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    TState,
    TConnection
  >,
  "insertConflictUpdate"
>;

/**
 * Exposes the builder type only when UPDATE can start.
 *
 * @remarks Updates can begin from the initial or CTE stage.
 *
 * @example
 * ```ts
 * type Db = UpdateCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, never, never, InitialBuilderState>;
 * ```
 */
export type UpdateCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = StageGate<
  DatabaseTypeParams<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    TState,
    TConnection
  >,
  StatementStartStage
>;

/**
 * Exposes the builder type only when SET assignments can be added.
 *
 * @remarks The state must already represent an UPDATE statement.
 *
 * @example
 * ```ts
 * type Db = SetCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, never, never, WithUpdate>;
 * ```
 */
export type SetCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = StageGate<
  DatabaseTypeParams<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    TState,
    TConnection
  >,
  "update" | "set"
>;

/**
 * Exposes the builder type only when DELETE can start.
 *
 * @remarks Deletes can begin from the initial or CTE stage.
 *
 * @example
 * ```ts
 * type Db = DeleteCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, never, never, InitialBuilderState>;
 * ```
 */
export type DeleteCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = StageGate<
  DatabaseTypeParams<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    TState,
    TConnection
  >,
  StatementStartStage
>;

/**
 * Exposes the builder type only when RETURNING can be called.
 *
 * @remarks The allowed stages cover DML statements and certain conflict branches.
 *
 * @example
 * ```ts
 * type Db = ReturningCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, never, never, WithDelete>;
 * ```
 */
export type ReturningCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> =
  HasStage<TState, CanReturnDirectStage> extends true
    ? DbOf<
        DatabaseTypeParams<
          TSchema,
          TRegisteredSources,
          TSources,
          TDefaultColumns,
          TSelectedColumns,
          TState,
          TConnection
        >
      >
    : TState["stage"] extends "insertConflictAction"
      ? DbOf<
          DatabaseTypeParams<
            TSchema,
            TRegisteredSources,
            TSources,
            TDefaultColumns,
            TSelectedColumns,
            TState,
            TConnection
          >
        >
      : TState["stage"] extends "where"
        ? TState["hasSelect"] extends false
          ? DbOf<
              DatabaseTypeParams<
                TSchema,
                TRegisteredSources,
                TSources,
                TDefaultColumns,
                TSelectedColumns,
                TState,
                TConnection
              >
            >
          : never
        : never;

/**
 * Exposes the builder type only when JOIN can be added.
 *
 * @remarks The current state must already have a FROM clause.
 *
 * @example
 * ```ts
 * type Db = JoinCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, string, string, WithFrom<InitialBuilderState>>;
 * ```
 */
export type JoinCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = TState["hasFrom"] extends true
  ? HasStage<TState, JoinableStage> extends true
    ? DbOf<
        DatabaseTypeParams<
          TSchema,
          TRegisteredSources,
          TSources,
          TDefaultColumns,
          TSelectedColumns,
          TState,
          TConnection
        >
      >
    : never
  : never;

/**
 * Exposes the builder type only while a JOIN predicate is pending.
 *
 * @remarks Used for on/using helpers before the join is finalized.
 *
 * @example
 * ```ts
 * type Db = JoinPredicateDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, string, string, WithPendingJoin<WithFrom<InitialBuilderState>>>;
 * ```
 */
export type JoinPredicateDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = StageGate<
  DatabaseTypeParams<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    TState,
    TConnection
  >,
  JoinPredicateStage
>;

/**
 * Exposes the builder type only when WHERE can be added.
 *
 * @remarks The gate covers SELECT, UPDATE, and DELETE branches.
 *
 * @example
 * ```ts
 * type Db = WhereCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, string, string, WithSelect<WithFrom<InitialBuilderState>>>;
 * ```
 */
export type WhereCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = TState["hasSelect"] extends true
  ? HasStage<TState, CanSelectWhereStage> extends true
    ? DbOf<
        DatabaseTypeParams<
          TSchema,
          TRegisteredSources,
          TSources,
          TDefaultColumns,
          TSelectedColumns,
          TState,
          TConnection
        >
      >
    : never
  : HasStage<TState, CanDmlWhereStage> extends true
    ? DbOf<
        DatabaseTypeParams<
          TSchema,
          TRegisteredSources,
          TSources,
          TDefaultColumns,
          TSelectedColumns,
          TState,
          TConnection
        >
      >
    : never;

/**
 * Exposes the builder type only when GROUP BY can be added.
 *
 * @remarks Grouping is only valid after a select list exists.
 *
 * @example
 * ```ts
 * type Db = GroupByCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, string, string, WithSelect<WithFrom<InitialBuilderState>>>;
 * ```
 */
export type GroupByCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = TState["hasSelect"] extends true
  ? HasStage<TState, CanGroupByStage> extends true
    ? DbOf<
        DatabaseTypeParams<
          TSchema,
          TRegisteredSources,
          TSources,
          TDefaultColumns,
          TSelectedColumns,
          TState,
          TConnection
        >
      >
    : never
  : never;

/**
 * Exposes the builder type only when HAVING can be added.
 *
 * @remarks HAVING requires a grouped query state.
 *
 * @example
 * ```ts
 * type Db = HavingCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, string, string, WithGroupBy<WithSelect<WithFrom<InitialBuilderState>>>>;
 * ```
 */
export type HavingCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = TState["hasGroupBy"] extends true
  ? HasStage<TState, CanHavingStage> extends true
    ? DbOf<
        DatabaseTypeParams<
          TSchema,
          TRegisteredSources,
          TSources,
          TDefaultColumns,
          TSelectedColumns,
          TState,
          TConnection
        >
      >
    : never
  : never;

/**
 * Exposes the builder type only when ORDER BY can be added.
 *
 * @remarks Ordering stays available across the post-select pipeline.
 *
 * @example
 * ```ts
 * type Db = OrderByCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, string, string, WithSelect<WithFrom<InitialBuilderState>>>;
 * ```
 */
export type OrderByCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = TState["hasSelect"] extends true
  ? HasStage<TState, CanOrderByStage> extends true
    ? DbOf<
        DatabaseTypeParams<
          TSchema,
          TRegisteredSources,
          TSources,
          TDefaultColumns,
          TSelectedColumns,
          TState,
          TConnection
        >
      >
    : never
  : never;

/**
 * Exposes the builder type only when LIMIT can be added.
 *
 * @remarks LIMIT is allowed once per completed select pipeline.
 *
 * @example
 * ```ts
 * type Db = LimitCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, string, string, WithOrderBy<WithSelect<WithFrom<InitialBuilderState>>>>;
 * ```
 */
export type LimitCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = TState["hasSelect"] extends true
  ? TState["hasLimit"] extends false
    ? HasStage<TState, CanLimitStage> extends true
      ? DbOf<
          DatabaseTypeParams<
            TSchema,
            TRegisteredSources,
            TSources,
            TDefaultColumns,
            TSelectedColumns,
            TState,
            TConnection
          >
        >
      : never
    : never
  : never;

/**
 * Exposes the builder type only when OFFSET can be added.
 *
 * @remarks OFFSET is allowed once per completed select pipeline.
 *
 * @example
 * ```ts
 * type Db = OffsetCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, string, string, WithOrderBy<WithSelect<WithFrom<InitialBuilderState>>>>;
 * ```
 */
export type OffsetCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = TState["hasSelect"] extends true
  ? TState["hasOffset"] extends false
    ? HasStage<TState, CanOffsetStage> extends true
      ? DbOf<
          DatabaseTypeParams<
            TSchema,
            TRegisteredSources,
            TSources,
            TDefaultColumns,
            TSelectedColumns,
            TState,
            TConnection
          >
        >
      : never
    : never
  : never;

/**
 * Exposes the builder type only when row locks can be added.
 *
 * @remarks Lock clauses are only valid near the end of a select pipeline.
 *
 * @example
 * ```ts
 * type Db = SelectLockCallableDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, string, string, WithOffset<WithSelect<WithFrom<InitialBuilderState>>>>;
 * ```
 */
export type SelectLockCallableDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = TState["hasSelect"] extends true
  ? HasStage<TState, CanSelectLockStage> extends true
    ? DbOf<
        DatabaseTypeParams<
          TSchema,
          TRegisteredSources,
          TSources,
          TDefaultColumns,
          TSelectedColumns,
          TState,
          TConnection
        >
      >
    : never
  : never;

/**
 * Exposes the builder type only when a complete select query exists.
 *
 * @remarks Used for accessors that require a finished SELECT statement.
 *
 * @example
 * ```ts
 * type Db = CompleteSelectDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, string, string, WithSelect<WithFrom<InitialBuilderState>>>;
 * ```
 */
export type CompleteSelectDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = TState["isCompleteSelectQuery"] extends true
  ? DbOf<
      DatabaseTypeParams<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >
    >
  : never;

/**
 * Exposes the builder type only when any complete query exists.
 *
 * @remarks DML and select completion both satisfy this gate.
 *
 * @example
 * ```ts
 * type Db = CompleteQueryDatabaseType<AnyDatabaseSchema | undefined, AnySourceColumnMap, AnySourceColumnMap, string, string, WithDelete>;
 * ```
 */
export type CompleteQueryDatabaseType<
  TSchema extends AnyDatabaseSchema | undefined,
  TRegisteredSources extends AnySourceColumnMap,
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
  TState extends AnyBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> = TState["isCompleteQuery"] extends true
  ? DbOf<
      DatabaseTypeParams<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >
    >
  : never;

/**
 * Represents a source map with no entries.
 *
 * @remarks Used for builders before any source has been introduced.
 *
 * @example
 * ```ts
 * type Sources = EmptySourceColumnMap;
 * ```
 */
export type EmptySourceColumnMap = Record<never, never>;
/**
 * Represents a fully widened database instance type.
 *
 * @remarks Useful when runtime helpers need to accept any builder instance.
 *
 * @example
 * ```ts
 * type Db = AnyDatabaseInstance;
 * ```
 */
export type AnyDatabaseInstance = DatabaseType<
  AnyDatabaseSchema | undefined,
  AnySourceColumnMap,
  AnySourceColumnMap,
  string,
  string,
  AnyBuilderState,
  AnyDatabaseConnection | undefined
>;

/**
 * Accepts a predicate or a predicate-building callback.
 *
 * @remarks Callback forms receive a scoped {@link BuilderContext}.
 *
 * @example
 * ```ts
 * type Input = PredicateResolverInput<{ users: "id" }, never>;
 * ```
 */
export type PredicateResolverInput<
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
> =
  | PredicateInput
  | ((
      db: BuilderContext<SelectedColumn<TSources, TDefaultColumns>>,
    ) => PredicateInput);

/**
 * Narrows the left side of an ON column comparison.
 *
 * @remarks Join comparisons always use visible selected columns.
 *
 * @example
 * ```ts
 * type Left = JoinOnLeft<{ users: "id" }>;
 * ```
 */
export type JoinOnLeft<TSources extends AnySourceColumnMap> = SelectedColumn<
  TSources,
  never
>;
/**
 * Narrows the right side of an ON column comparison.
 *
 * @remarks The right side may be another column or a non-string primitive.
 *
 * @example
 * ```ts
 * type Right = JoinOnRight<{ users: "id" }>;
 * ```
 */
export type JoinOnRight<TSources extends AnySourceColumnMap> =
  | SelectedColumn<TSources, never>
  | Exclude<Primitive, string>;
/**
 * Accepts one or many ORDER BY inputs.
 *
 * @remarks Inputs may target columns or selected aliases.
 *
 * @example
 * ```ts
 * type Input = OrderByInput<{ users: "id" }, never, "total">;
 * ```
 */
export type OrderByInput<
  TSources extends AnySourceColumnMap,
  TDefaultColumns extends string,
  TSelectedColumns extends string,
> =
  | OrderableColumn<TSources, TDefaultColumns, TSelectedColumns>
  | Array<OrderableColumn<TSources, TDefaultColumns, TSelectedColumns>>;

/**
 * Rebuilds a {@link BuilderState} with a different stage name.
 *
 * @remarks Other state flags are preserved as-is.
 *
 * @example
 * ```ts
 * type State = WithStage<InitialBuilderState, "cte">;
 * ```
 */
export type WithStage<
  TState extends AnyBuilderState,
  TStage extends QueryStage,
> = BuilderState<
  TStage,
  TState["hasFrom"],
  TState["hasSelect"],
  TState["hasGroupBy"],
  TState["hasLimit"],
  TState["hasOffset"]
>;

/**
 * Marks a state as having entered the FROM stage.
 *
 * @remarks The resulting state always records a source.
 *
 * @example
 * ```ts
 * type State = WithFrom<InitialBuilderState>;
 * ```
 */
export type WithFrom<TState extends AnyBuilderState> = BuilderState<
  "from",
  true,
  TState["hasSelect"],
  TState["hasGroupBy"],
  TState["hasLimit"],
  TState["hasOffset"]
>;

/**
 * Represents the state after entering a CTE clause.
 *
 * @remarks No FROM or SELECT clause has been fixed yet.
 *
 * @example
 * ```ts
 * type State = WithCte;
 * ```
 */
export type WithCte = BuilderState<"cte", false, false, false, false, false>;
/**
 * Represents the state after entering a DISTINCT clause.
 *
 * @remarks Other state flags are preserved from the prior state.
 *
 * @example
 * ```ts
 * type State = WithDistinct<WithFrom<InitialBuilderState>>;
 * ```
 */
export type WithDistinct<TState extends AnyBuilderState> = WithStage<
  TState,
  "distinct"
>;
/**
 * Represents the state after entering INSERT INTO.
 *
 * @remarks Used for INSERT statement progression.
 *
 * @example
 * ```ts
 * type State = WithInsertInto;
 * ```
 */
export type WithInsertInto = BuilderState<
  "insertInto",
  false,
  false,
  false,
  false,
  false
>;
/**
 * Represents the state after listing INSERT columns.
 *
 * @remarks Used before values or select-based inserts are provided.
 *
 * @example
 * ```ts
 * type State = WithInsertColumns;
 * ```
 */
export type WithInsertColumns = BuilderState<
  "insertColumns",
  false,
  false,
  false,
  false,
  false
>;
/**
 * Represents the state after adding INSERT values.
 *
 * @remarks Conflict handling may still follow from this state.
 *
 * @example
 * ```ts
 * type State = WithInsertValues;
 * ```
 */
export type WithInsertValues = BuilderState<
  "insertValues",
  false,
  false,
  false,
  false,
  false
>;
/**
 * Represents the state after adding an INSERT ... SELECT source.
 *
 * @remarks Conflict handling may still follow from this state.
 *
 * @example
 * ```ts
 * type State = WithInsertSelect;
 * ```
 */
export type WithInsertSelect = BuilderState<
  "insertSelect",
  false,
  false,
  false,
  false,
  false
>;
/**
 * Represents the state after setting an INSERT conflict target.
 *
 * @remarks A conflict action can follow from this state.
 *
 * @example
 * ```ts
 * type State = WithInsertConflictTarget;
 * ```
 */
export type WithInsertConflictTarget = BuilderState<
  "insertConflictTarget",
  false,
  false,
  false,
  false,
  false
>;
/**
 * Represents the state during INSERT conflict updates.
 *
 * @remarks Additional conflict predicates may still be added.
 *
 * @example
 * ```ts
 * type State = WithInsertConflictUpdate;
 * ```
 */
export type WithInsertConflictUpdate = BuilderState<
  "insertConflictUpdate",
  false,
  false,
  false,
  false,
  false
>;
/**
 * Represents the state after choosing an INSERT conflict action.
 *
 * @remarks The next step may be RETURNING or completion.
 *
 * @example
 * ```ts
 * type State = WithInsertConflictAction;
 * ```
 */
export type WithInsertConflictAction = BuilderState<
  "insertConflictAction",
  false,
  false,
  false,
  false,
  false
>;
/**
 * Represents the state after entering RETURNING.
 *
 * @remarks Used by DML statements that project changed rows.
 *
 * @example
 * ```ts
 * type State = WithReturning;
 * ```
 */
export type WithReturning = BuilderState<
  "returning",
  false,
  false,
  false,
  false,
  false
>;
/**
 * Represents the state after appending a JOIN clause.
 *
 * @remarks Additional joins or predicates may still follow.
 *
 * @example
 * ```ts
 * type State = WithJoin<WithFrom<InitialBuilderState>>;
 * ```
 */
export type WithJoin<TState extends AnyBuilderState> = WithStage<
  TState,
  "join"
>;
/**
 * Represents the state while a JOIN predicate is still pending.
 *
 * @remarks Used before ON or USING details are finalized.
 *
 * @example
 * ```ts
 * type State = WithPendingJoin<WithFrom<InitialBuilderState>>;
 * ```
 */
export type WithPendingJoin<TState extends AnyBuilderState> = WithStage<
  TState,
  "joinPending"
>;
/**
 * Represents the state after a pending JOIN has enough data to continue.
 *
 * @remarks The join may now flow into further clauses.
 *
 * @example
 * ```ts
 * type State = WithPendingJoinReady<WithFrom<InitialBuilderState>>;
 * ```
 */
export type WithPendingJoinReady<TState extends AnyBuilderState> = WithStage<
  TState,
  "joinPendingReady"
>;
/**
 * Represents the state after entering WHERE.
 *
 * @remarks Additional predicates may still be chained.
 *
 * @example
 * ```ts
 * type State = WithWhere<WithSelect<WithFrom<InitialBuilderState>>>;
 * ```
 */
export type WithWhere<TState extends AnyBuilderState> = WithStage<
  TState,
  "where"
>;
/**
 * Represents the state after entering HAVING.
 *
 * @remarks Additional grouped predicates may still be chained.
 *
 * @example
 * ```ts
 * type State = WithHaving<WithGroupBy<WithSelect<WithFrom<InitialBuilderState>>>>;
 * ```
 */
export type WithHaving<TState extends AnyBuilderState> = WithStage<
  TState,
  "having"
>;
/**
 * Represents the state after entering ORDER BY.
 *
 * @remarks Additional ordering items may still be appended.
 *
 * @example
 * ```ts
 * type State = WithOrderBy<WithSelect<WithFrom<InitialBuilderState>>>;
 * ```
 */
export type WithOrderBy<TState extends AnyBuilderState> = WithStage<
  TState,
  "orderBy"
>;
/**
 * Represents the state after entering UPDATE.
 *
 * @remarks The next step is usually SET.
 *
 * @example
 * ```ts
 * type State = WithUpdate;
 * ```
 */
export type WithUpdate = BuilderState<
  "update",
  false,
  false,
  false,
  false,
  false
>;
/**
 * Represents the state after entering SET.
 *
 * @remarks WHERE or RETURNING may still follow.
 *
 * @example
 * ```ts
 * type State = WithSet;
 * ```
 */
export type WithSet = BuilderState<"set", false, false, false, false, false>;
/**
 * Represents the state after entering DELETE.
 *
 * @remarks WHERE or RETURNING may still follow.
 *
 * @example
 * ```ts
 * type State = WithDelete;
 * ```
 */
export type WithDelete = BuilderState<
  "delete",
  false,
  false,
  false,
  false,
  false
>;
/**
 * Represents the state after entering a compound query clause.
 *
 * @remarks Existing source and select flags are preserved.
 *
 * @example
 * ```ts
 * type State = WithCompound<WithSelect<WithFrom<InitialBuilderState>>>;
 * ```
 */
export type WithCompound<TState extends AnyBuilderState> = BuilderState<
  "compound",
  TState["hasFrom"],
  TState["hasSelect"],
  TState["hasGroupBy"],
  TState["hasLimit"],
  TState["hasOffset"]
>;

/**
 * Represents the state after entering SELECT.
 *
 * @remarks The resulting state always records a select list.
 *
 * @example
 * ```ts
 * type State = WithSelect<WithFrom<InitialBuilderState>>;
 * ```
 */
export type WithSelect<TState extends AnyBuilderState> = BuilderState<
  "select",
  TState["hasFrom"],
  true,
  TState["hasGroupBy"],
  TState["hasLimit"],
  TState["hasOffset"]
>;

/**
 * Represents the state after entering GROUP BY.
 *
 * @remarks The resulting state always records grouped output.
 *
 * @example
 * ```ts
 * type State = WithGroupBy<WithSelect<WithFrom<InitialBuilderState>>>;
 * ```
 */
export type WithGroupBy<TState extends AnyBuilderState> = BuilderState<
  "groupBy",
  TState["hasFrom"],
  TState["hasSelect"],
  true,
  TState["hasLimit"],
  TState["hasOffset"]
>;

/**
 * Represents the state after entering LIMIT.
 *
 * @remarks The resulting state records that a limit is already present.
 *
 * @example
 * ```ts
 * type State = WithLimit<WithSelect<WithFrom<InitialBuilderState>>>;
 * ```
 */
export type WithLimit<TState extends AnyBuilderState> = BuilderState<
  "limit",
  TState["hasFrom"],
  TState["hasSelect"],
  TState["hasGroupBy"],
  true,
  TState["hasOffset"]
>;

/**
 * Represents the state after entering OFFSET.
 *
 * @remarks The resulting state records that an offset is already present.
 *
 * @example
 * ```ts
 * type State = WithOffset<WithSelect<WithFrom<InitialBuilderState>>>;
 * ```
 */
export type WithOffset<TState extends AnyBuilderState> = BuilderState<
  "offset",
  TState["hasFrom"],
  TState["hasSelect"],
  TState["hasGroupBy"],
  TState["hasLimit"],
  true
>;

/**
 * Stores a predicate encoded for pending operation payloads.
 *
 * @remarks The value is safe to carry into compact pending-op serialization.
 *
 * @example
 * ```ts
 * type Predicate = PendingPredicate;
 * ```
 */
export type PendingPredicate = ReturnType<typeof buildPredicateForOp>;

/**
 * Describes pending DISTINCT operations.
 *
 * @remarks Distinct state may use a plain flag, columns, or expressions.
 *
 * @example
 * ```ts
 * const op: PendingDistinctOp = { op: "distinct" };
 * ```
 */
export type PendingDistinctOp =
  | { op: "distinct" }
  | { op: "distinctOnColumns"; cols: string[] }
  | { op: "distinctOnExprs"; exprs: unknown[] };

/**
 * Describes pending WHERE, HAVING, and ON predicate operations.
 *
 * @remarks Predicate chaining is preserved by distinct op tags.
 *
 * @example
 * ```ts
 * const op: PendingWhereOp = { op: "where", pred: {} as PendingPredicate };
 * ```
 */
export type PendingWhereOp =
  | { op: "where"; pred: PendingPredicate }
  | { op: "andWhere"; pred: PendingPredicate }
  | { op: "orWhere"; pred: PendingPredicate }
  | { op: "having"; pred: PendingPredicate }
  | { op: "andHaving"; pred: PendingPredicate }
  | { op: "orHaving"; pred: PendingPredicate }
  | { op: "on"; pred: PendingPredicate }
  | { op: "andOn"; pred: PendingPredicate }
  | { op: "orOn"; pred: PendingPredicate };

/**
 * Describes pending source and subquery operations.
 *
 * @remarks Source ops cover FROM and JOIN clauses for tables and subqueries.
 *
 * @example
 * ```ts
 * const op: PendingSourceOp = { op: "fromTable", table: "users" };
 * ```
 */
export type PendingSourceOp =
  | { op: "fromTable"; table: string }
  | { op: "fromTableAlias"; table: string; alias: string }
  | { op: "joinTable"; joinType: string; table: string }
  | { op: "joinTableAlias"; joinType: string; table: string; alias: string }
  | {
      op: "fromSubqueryBuilder";
      alias: string;
      rhsBuilder: AnyDatabaseInstance;
    }
  | { op: "fromSubqueryHandle"; alias: string; rhsHandle: string }
  | { op: "fromSubquery"; alias: string; query: unknown }
  | {
      op: "joinSubqueryBuilder";
      joinType: string;
      alias: string;
      rhsBuilder: AnyDatabaseInstance;
    }
  | {
      op: "joinSubqueryHandle";
      joinType: string;
      alias: string;
      rhsHandle: string;
    }
  | { op: "joinSubquery"; joinType: string; alias: string; query: unknown };

/**
 * Describes pending JOIN constraint operations.
 *
 * @remarks Constraints may be expressed as USING columns or ON column pairs.
 *
 * @example
 * ```ts
 * const op: PendingJoinConstraintOp = { op: "usingColumns", cols: ["id"] };
 * ```
 */
export type PendingJoinConstraintOp =
  | { op: "usingColumns"; cols: string[] }
  | { op: "onColumns"; pairs: Array<readonly [string, string]> };

/**
 * Describes pending CTE operations.
 *
 * @remarks CTE entries may refer to builders, handles, or serialized queries.
 *
 * @example
 * ```ts
 * const op: PendingQueryCteOp = { op: "withQuery", name: "recent_users", query: {} };
 * ```
 */
export type PendingQueryCteOp =
  | { op: "withBuilder"; name: string; rhsBuilder: AnyDatabaseInstance }
  | { op: "withHandle"; name: string; rhsHandle: string }
  | { op: "withQuery"; name: string; query: unknown }
  | {
      op: "withRecursiveBuilder";
      name: string;
      rhsBuilder: AnyDatabaseInstance;
    }
  | { op: "withRecursiveHandle"; name: string; rhsHandle: string }
  | {
      op: "withRecursiveQuery";
      name: string;
      query: unknown;
      columns?: string[];
    };

/**
 * Describes pending INSERT operations.
 *
 * @remarks Insert ops cover targets, values, conflicts, and insert-select sources.
 *
 * @example
 * ```ts
 * const op: PendingInsertOp = { op: "insertInto", table: "users" };
 * ```
 */
export type PendingInsertOp =
  | { op: "insertInto"; table: string }
  | { op: "insertColumns"; cols: string[] }
  | { op: "valuesInsert"; columnNames: string[]; rows: unknown[][] }
  | { op: "onConflictColumns"; cols: string[] }
  | { op: "onConflictConstraint"; name: string }
  | { op: "doNothing" }
  | { op: "doUpdateSet"; assignments: [string, unknown][] }
  | { op: "conflictWhere"; pred: PendingPredicate }
  | { op: "insertSelectHandle"; rhsHandle: string }
  | { op: "insertSelect"; query: unknown };

/**
 * Describes pending UPDATE, DELETE, and SET operations.
 *
 * @remarks These operations cover the mutable parts of DML statements.
 *
 * @example
 * ```ts
 * const op: PendingDmlOp = { op: "update", table: "users" };
 * ```
 */
export type PendingDmlOp =
  | { op: "update"; table: string }
  | { op: "deleteFrom"; table: string }
  | { op: "set"; assignments: [string, unknown][] };

/**
 * Describes pending compound-query operations.
 *
 * @remarks Compound ops cover unions, intersections, and exceptions.
 *
 * @example
 * ```ts
 * const op: PendingCompoundOp = { op: "union", query: {} };
 * ```
 */
export type PendingCompoundOp =
  | { op: "unionBuilder"; rhsBuilder: AnyDatabaseInstance }
  | { op: "unionHandle"; rhsHandle: string }
  | { op: "union"; query: unknown }
  | { op: "unionAllBuilder"; rhsBuilder: AnyDatabaseInstance }
  | { op: "unionAllHandle"; rhsHandle: string }
  | { op: "unionAll"; query: unknown }
  | { op: "intersectBuilder"; rhsBuilder: AnyDatabaseInstance }
  | { op: "intersectHandle"; rhsHandle: string }
  | { op: "intersect"; query: unknown }
  | { op: "exceptBuilder"; rhsBuilder: AnyDatabaseInstance }
  | { op: "exceptHandle"; rhsHandle: string }
  | { op: "except"; query: unknown };

/**
 * Stores one aliased expression in a pending projection payload.
 *
 * @remarks Bare expressions are marked when no explicit alias keyword is needed.
 *
 * @example
 * ```ts
 * const entry: PendingAliasedEntry = { alias: "total", expr: {} };
 * ```
 */
export type PendingAliasedEntry = {
  alias: string;
  expr: unknown;
  bare?: boolean;
};

/**
 * Describes pending projection operations.
 *
 * @remarks Projection ops cover SELECT and RETURNING payloads.
 *
 * @example
 * ```ts
 * const op: PendingProjectionOp = { op: "selectColumns", cols: ["id"] };
 * ```
 */
export type PendingProjectionOp =
  | { op: "selectAliased"; entries: PendingAliasedEntry[] }
  | { op: "selectColumns"; cols: string[] }
  | { op: "returningAliased"; entries: PendingAliasedEntry[] }
  | { op: "returningColumns"; cols: string[] };

/**
 * Describes pending ORDER BY and GROUP BY operations.
 *
 * @remarks Ordering metadata preserves optional direction and null ordering.
 *
 * @example
 * ```ts
 * const op: PendingOrderOp = { op: "orderByColumns", cols: ["id"] };
 * ```
 */
export type PendingOrderOp =
  | {
      op: "orderBy";
      col: string;
      direction?: string;
      nullOrder?: string;
    }
  | { op: "orderByColumns"; cols: string[] }
  | { op: "groupBy"; cols: string[] };

/**
 * Describes pending pagination and row-lock operations.
 *
 * @remarks Pagination ops also carry select-lock modifiers.
 *
 * @example
 * ```ts
 * const op: PendingPaginationOp = { op: "limit", count: 10 };
 * ```
 */
export type PendingPaginationOp =
  | { op: "limit"; count: number }
  | { op: "offset"; count: number }
  | { op: "forUpdate" }
  | { op: "forShare" }
  | { op: "noWait" }
  | { op: "skipLocked" };

/**
 * Unions every pending operation payload.
 *
 * @remarks This is the expanded JSON-friendly representation.
 *
 * @example
 * ```ts
 * const op: PendingOp = { op: "limit", count: 10 };
 * ```
 */
export type PendingOp =
  | PendingDistinctOp
  | PendingWhereOp
  | PendingSourceOp
  | PendingJoinConstraintOp
  | PendingQueryCteOp
  | PendingInsertOp
  | PendingDmlOp
  | PendingCompoundOp
  | PendingProjectionOp
  | PendingOrderOp
  | PendingPaginationOp;

/**
 * Unions every compact pending operation tuple.
 *
 * @remarks This is the minimized representation sent to the runtime.
 *
 * @example
 * ```ts
 * const op: CompactPendingOp = ["d"];
 * ```
 */
export type CompactPendingOp =
  | readonly ["w", PendingPredicate]
  | readonly ["d"]
  | readonly ["dc", string[]]
  | readonly ["de", unknown[]]
  | readonly ["aw", PendingPredicate]
  | readonly ["ow", PendingPredicate]
  | readonly ["h", PendingPredicate]
  | readonly ["ah", PendingPredicate]
  | readonly ["oh", PendingPredicate]
  | readonly ["ft", string]
  | readonly ["fta", string, string]
  | readonly ["jt", string, string]
  | readonly ["jta", string, string, string]
  | readonly ["on", PendingPredicate]
  | readonly ["aon", PendingPredicate]
  | readonly ["oon", PendingPredicate]
  | readonly ["uc", string[]]
  | readonly ["oc", Array<readonly [string, string]>]
  | readonly ["wh", string, string]
  | readonly ["wq", string, unknown]
  | readonly ["wrh", string, string]
  | readonly ["wrq", string, unknown]
  | {
      op: "withRecursiveQuery";
      name: string;
      query: unknown;
      columns: string[];
    }
  | readonly ["ii", string]
  | readonly ["ic", string[]]
  | readonly ["vi", string[], unknown[][]]
  | readonly ["upd", string]
  | readonly ["del", string]
  | readonly ["set", [string, unknown][]]
  | readonly ["ict", string[]]
  | readonly ["icn", string]
  | readonly ["idn"]
  | readonly ["idu", [string, unknown][]]
  | readonly ["icw", PendingPredicate]
  | readonly ["sa", PendingAliasedEntry[]]
  | readonly ["sc", string[]]
  | readonly ["ra", PendingAliasedEntry[]]
  | readonly ["rc", string[]]
  | readonly ["ob", string, string | null, string | null]
  | readonly ["obc", string[]]
  | readonly ["gb", string[]]
  | readonly ["l", number]
  | readonly ["o", number]
  | readonly ["fu"]
  | readonly ["fs"]
  | readonly ["nw"]
  | readonly ["sl"]
  | readonly ["unh", string]
  | readonly ["un", unknown]
  | readonly ["uah", string]
  | readonly ["ua", unknown]
  | readonly ["ixh", string]
  | readonly ["ix", unknown]
  | readonly ["exh", string]
  | readonly ["ex", unknown]
  | readonly ["fsqh", string, string]
  | readonly ["fsq", string, unknown]
  | readonly ["jsqh", string, string, string]
  | readonly ["jsq", string, string, unknown]
  | readonly ["ish", string]
  | readonly ["is", unknown]
  | readonly ["whi", string, string, CompactPendingOp[]]
  | readonly ["wrhi", string, string, CompactPendingOp[]]
  | readonly ["fsqi", string, string, CompactPendingOp[]]
  | readonly ["jsqi", string, string, string, CompactPendingOp[]]
  | readonly ["uni", string, CompactPendingOp[]]
  | readonly ["uai", string, CompactPendingOp[]]
  | readonly ["ixi", string, CompactPendingOp[]]
  | readonly ["exi", string, CompactPendingOp[]];

/**
 * Stores compile-phase timing and mode diagnostics for node-query.
 *
 * @remarks These measurements cover pending-op serialization and bundle compilation.
 *
 * @example
 * ```ts
 * const diagnostics: NodeQueryCompileDiagnostics = {
 * 	pendingOpsMaterializeMs: 0,
 * 	pendingOpsSerializeMs: 0,
 * 	pendingOpsApplyMs: 0,
 * 	compileBundleMs: 0,
 * 	pendingOpCount: 0,
 * 	applyMode: "none",
 * };
 * ```
 */
export type NodeQueryCompileDiagnostics = {
  pendingOpsMaterializeMs: number;
  pendingOpsSerializeMs: number;
  pendingOpsApplyMs: number;
  compileBundleMs: number;
  pendingOpCount: number;
  applyMode: "binary" | "json" | "none" | "mixed";
};

/**
 * Stores build-phase timing and counter diagnostics for node-query.
 *
 * @remarks These measurements cover builder mutations, callbacks, and runtime calls.
 *
 * @example
 * ```ts
 * const diagnostics: NodeQueryBuildDiagnostics = {
 * 	directMutationMs: 0,
 * 	cloneWithOwnedHandleMs: 0,
 * 	callbackResolveMs: 0,
 * 	builderContextCreateMs: 0,
 * 	exprNodeBuildMs: 0,
 * 	predicateBuildMs: 0,
 * 	jsonSerializeMs: 0,
 * 	runtimeCallMs: 0,
 * 	runtimeSourceMs: 0,
 * 	runtimeSelectMs: 0,
 * 	runtimePredicateMs: 0,
 * 	runtimeCteMs: 0,
 * 	runtimeOrderMs: 0,
 * 	runtimePaginationMs: 0,
 * 	directMutationCount: 0,
 * 	runtimeCallCount: 0,
 * 	builderContextCacheHits: 0,
 * 	builderContextCacheMisses: 0,
 * 	exprNodeCount: 0,
 * 	predicateBuildCount: 0,
 * 	jsonSerializeCount: 0,
 * };
 * ```
 */
export type NodeQueryBuildDiagnostics = {
  directMutationMs: number;
  cloneWithOwnedHandleMs: number;
  callbackResolveMs: number;
  builderContextCreateMs: number;
  exprNodeBuildMs: number;
  predicateBuildMs: number;
  jsonSerializeMs: number;
  runtimeCallMs: number;
  runtimeSourceMs: number;
  runtimeSelectMs: number;
  runtimePredicateMs: number;
  runtimeCteMs: number;
  runtimeOrderMs: number;
  runtimePaginationMs: number;
  directMutationCount: number;
  runtimeCallCount: number;
  builderContextCacheHits: number;
  builderContextCacheMisses: number;
  exprNodeCount: number;
  predicateBuildCount: number;
  jsonSerializeCount: number;
};

/**
 * Names each timed step in compile diagnostics.
 *
 * @remarks Used by compile diagnostic collectors when recording samples.
 *
 * @example
 * ```ts
 * const step: NodeQueryCompileDiagnosticsStep = "compileBundleMs";
 * ```
 */
export type NodeQueryCompileDiagnosticsStep =
  | "pendingOpsMaterializeMs"
  | "pendingOpsSerializeMs"
  | "pendingOpsApplyMs"
  | "compileBundleMs";

/**
 * Names each timed step in build diagnostics.
 *
 * @remarks Used by build diagnostic collectors when recording samples.
 *
 * @example
 * ```ts
 * const step: NodeQueryBuildDiagnosticsStep = "runtimeCallMs";
 * ```
 */
export type NodeQueryBuildDiagnosticsStep =
  | "directMutationMs"
  | "cloneWithOwnedHandleMs"
  | "callbackResolveMs"
  | "builderContextCreateMs"
  | "exprNodeBuildMs"
  | "predicateBuildMs"
  | "jsonSerializeMs"
  | "runtimeCallMs"
  | "runtimeSourceMs"
  | "runtimeSelectMs"
  | "runtimePredicateMs"
  | "runtimeCteMs"
  | "runtimeOrderMs"
  | "runtimePaginationMs";

/**
 * Names each counter tracked by build diagnostics.
 *
 * @remarks Used when incrementing non-duration build metrics.
 *
 * @example
 * ```ts
 * const counter: NodeQueryBuildDiagnosticsCounter = "runtimeCallCount";
 * ```
 */
export type NodeQueryBuildDiagnosticsCounter =
  | "directMutationCount"
  | "runtimeCallCount"
  | "builderContextCacheHits"
  | "builderContextCacheMisses"
  | "exprNodeCount"
  | "predicateBuildCount"
  | "jsonSerializeCount";

/**
 * Records compile diagnostics into a shared sample object.
 *
 * @remarks Callers provide a clock and step-recording helpers.
 *
 * @example
 * ```ts
 * const collector = {} as NodeQueryCompileDiagnosticsCollector;
 * collector.recordStep("compileBundleMs", 1);
 * ```
 */
export type NodeQueryCompileDiagnosticsCollector = {
  now: () => number;
  sample: NodeQueryCompileDiagnostics;
  recordStep(step: NodeQueryCompileDiagnosticsStep, durationMs: number): void;
  recordApplyMode(
    mode: Exclude<NodeQueryCompileDiagnostics["applyMode"], "mixed">,
  ): void;
};

/**
 * Records build diagnostics into a shared sample object.
 *
 * @remarks Callers provide timing and counter helpers for build instrumentation.
 *
 * @example
 * ```ts
 * const collector = {} as NodeQueryBuildDiagnosticsCollector;
 * collector.incrementCounter("runtimeCallCount");
 * ```
 */
export type NodeQueryBuildDiagnosticsCollector = {
  now: () => number;
  sample: NodeQueryBuildDiagnostics;
  recordStep(step: NodeQueryBuildDiagnosticsStep, durationMs: number): void;
  incrementCounter(counter: NodeQueryBuildDiagnosticsCounter): void;
};
