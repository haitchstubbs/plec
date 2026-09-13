import { encode } from '@msgpack/msgpack';
import {
  ARITHMETIC_OPERATORS,
  COMPARISON_OPERATORS,
  DIALECTS,
  type Dialect,
  SUPPORTED_FUNCTIONS,
  type SupportedFunction,
} from '#core';
import type {
  Primitive,
  SqlIdentifier,
  SqlQuery,
  SqlRaw,
  SqlValue,
} from '#types';
import { QueryValidationError } from '../main/errors';
import {
  bindArraySqlValueMethod,
  bindBinarySqlValueMethod,
  bindConditionMethod,
  bindExistsSqlMethod,
  bindRuntimeMethod,
  bindTernarySqlValueMethod,
  bindUnarySqlValueMethod,
  getBinding,
} from './get-runtime-binding';
import { hash64Hex } from './hash-64-hex';
import { makeDeferredQuery } from './make-deferred-query';
import { reviveCompileBundle } from './revivers/revive-compile-bundle';
import { revivePrimitive } from './revivers/revive-primitive';
import { revivePrimitiveList } from './revivers/revive-primitive-list';
import { reviveQuery } from './revivers/revive-query';
import { serializeImmediateSqlValue } from './serializers/serialize-immediate-sql-value';
import { serializeQuery } from './serializers/serialize-query';
import { serializeSqlValue } from './serializers/serialize-sql-value';
import type { WireQuery } from './types';

// Process-scoped expression dialect used by direct expression helper calls.
// Database instances refresh this as query-building operations run.
let activeRuntimeDialect: Dialect | undefined;

function resolveRuntimeDialect(overrideDialect?: Dialect): Dialect {
  return normalizeRuntimeDialect(
    overrideDialect ?? activeRuntimeDialect ?? 'postgres',
  ) as Dialect;
}

function normalizeRuntimeDialect(dialect: Dialect): Dialect {
  return dialect.trim().toLowerCase() as Dialect;
}

function validateFunctionForDialect(
  name: SupportedFunction,
  dialect: Dialect,
): string {
  const normalized = name.trim().toUpperCase() as SupportedFunction;
  if (!DIALECTS.has(dialect) || !SUPPORTED_FUNCTIONS.has(normalized)) {
    throw new Error(
      `Dialect "${dialect}" does not support function "${normalized}" in this builder.`,
    );
  }
  return normalized;
}

function validateOperatorForDialect(
  operator: string,
  dialect: Dialect,
  kind: 'comparison' | 'arithmetic',
): string {
  const normalized = operator.trim();
  if (!DIALECTS.has(dialect)) {
    throw new Error(
      `Dialect "${dialect}" does not support operator "${normalized}" in this builder.`,
    );
  }

  const supported =
    kind === 'comparison'
      ? (COMPARISON_OPERATORS as ReadonlySet<string>).has(normalized)
      : (ARITHMETIC_OPERATORS as ReadonlySet<string>).has(normalized);
  if (!supported) {
    throw new Error(
      `Dialect "${dialect}" does not support operator "${normalized}" in this builder.`,
    );
  }
  return normalized;
}

export function setActiveRuntimeDialect(dialect: Dialect): void {
  activeRuntimeDialect = dialect;
}

export function getActiveRuntimeDialect(): Dialect | undefined {
  return activeRuntimeDialect;
}

export function clearActiveRuntimeDialect(): void {
  activeRuntimeDialect = undefined;
}

function rethrowStructuredValidationError(err: unknown): never {
  if (err instanceof Error) {
    try {
      const parsed = JSON.parse(err.message) as {
        kind?: string;
        errors?: Array<{
          code: string;
          message: string;
          feature: string;
        }>;
      };
      if (
        parsed.kind === 'ValidationError' &&
        Array.isArray(parsed.errors)
      ) {
        throw new QueryValidationError(
          parsed.errors.map((entry) => ({
            code: entry.code,
            message: entry.message,
            feature:
              entry.feature as import('#types').UnsupportedFeature,
          })),
        );
      }
    } catch (innerErr) {
      if (innerErr instanceof QueryValidationError) {
        throw innerErr;
      }
    }
  }

  throw err;
}

export function runtimeIdentifier(parts: string[]): SqlIdentifier {
  return getBinding().identifier(parts) as unknown as SqlIdentifier;
}

export function runtimeRaw(text: string): SqlRaw {
  return getBinding().raw(text) as unknown as SqlRaw;
}

export function runtimeJoin(
  items: Array<SqlQuery | SqlIdentifier | SqlRaw>,
  separator?: string,
): SqlQuery {
  return reviveQuery(
    getBinding().join(
      items.map((item) => serializeImmediateSqlValue(item)),
      separator,
    ),
  );
}

export function runtimeSql(
  strings: readonly string[],
  exprs: SqlValue[],
): SqlQuery {
  return reviveQuery(
    getBinding().sql(
      [...strings],
      exprs.map((expr) => serializeImmediateSqlValue(expr)),
    ),
  );
}

export function runtimeDeferredSql(
  strings: readonly string[],
  exprs: SqlValue[],
): SqlQuery {
  const stringChunks = [...strings];
  const serializedExprs = exprs.map((expr) => serializeSqlValue(expr));
  return makeDeferredQuery(
    {
      type: 'sqlTemplate',
      strings: stringChunks,
      exprs: serializedExprs,
    },
    () => runtimeSql(stringChunks, exprs),
  );
}

export function runtimeRef(parts: string[]): SqlIdentifier {
  return getBinding().refIdentifier(parts) as unknown as SqlIdentifier;
}

export function runtimeCompilePostgres(query: SqlQuery): SqlQuery {
  return reviveQuery(
    getBinding().compilePostgres(serializeQuery(query)),
  );
}

export function runtimeCompileQuery(
  query: SqlQuery,
  dialect: string,
): SqlQuery {
  return reviveQuery(
    getBinding().compileQuery(serializeQuery(query), dialect),
  );
}

export function runtimeCmp(
  left: SqlValue,
  operator: string,
  right: SqlValue,
  dialect?: string,
): SqlQuery {
  const resolvedDialect = resolveRuntimeDialect(
    dialect as Dialect | undefined,
  );
  const normalized = validateOperatorForDialect(
    operator,
    resolvedDialect,
    'comparison',
  );
  return reviveQuery(
    getBinding().cmp(
      serializeSqlValue(left),
      normalized,
      serializeSqlValue(right),
      resolvedDialect,
    ),
  );
}

export function runtimeFnCall(
  name: SupportedFunction,
  args: SqlValue[],
  dialect?: Dialect,
): SqlQuery {
  const resolvedDialect = resolveRuntimeDialect(dialect);
  const normalizedName = validateFunctionForDialect(
    name,
    resolvedDialect,
  );
  return reviveQuery(
    getBinding().fnCall(
      normalizedName,
      args.map(serializeSqlValue),
      resolvedDialect,
    ),
  );
}

export function runtimeOverClause(
  query: SqlQuery,
  partitionBy: SqlQuery[],
  orderBy: Array<{
    expression: SqlQuery;
    direction?: 'ASC' | 'DESC';
    nulls?: 'FIRST' | 'LAST';
  }>,
  dialect: string,
): SqlQuery {
  return reviveQuery(
    getBinding().overClause(
      serializeSqlValue(query) as WireQuery,
      partitionBy.map((q) => serializeSqlValue(q) as WireQuery),
      orderBy.map((item) => ({
        expression: serializeSqlValue(item.expression),
        direction: item.direction,
        nulls: item.nulls,
      })),
      dialect,
    ),
  );
}

export function runtimeScalarCase(
  branches: Array<[SqlValue, SqlValue]>,
  elseVal?: SqlValue,
): SqlQuery {
  return reviveQuery(
    getBinding().scalarCase(
      branches.map(
        ([when, then]) =>
          [serializeSqlValue(when), serializeSqlValue(then)] as [
            unknown,
            unknown,
          ],
      ),
      elseVal !== undefined ? serializeSqlValue(elseVal) : undefined,
    ),
  );
}

export function runtimeArithBinary(
  left: SqlValue,
  operator: string,
  right: SqlValue,
  dialect?: string,
): SqlQuery {
  const resolvedDialect = resolveRuntimeDialect(
    dialect as Dialect | undefined,
  );
  const normalized = validateOperatorForDialect(
    operator,
    resolvedDialect,
    'arithmetic',
  );
  return reviveQuery(
    getBinding().arithBinary(
      serializeSqlValue(left),
      normalized,
      serializeSqlValue(right),
      resolvedDialect,
    ),
  );
}

// ─── Builder bridge functions ─────────────────────────────────────────────────

export function runtimeBuilderNew(dialect?: string): string {
  if (dialect) {
    setActiveRuntimeDialect(dialect as Dialect);
  }
  return getBinding().builderNew(dialect);
}
// Runtime condition methods
export const runtimeAnd = bindConditionMethod('and');
export const runtimeOr = bindConditionMethod('or');
export const runtimeIsNull = bindUnarySqlValueMethod('isNull');
export const runtimeIsNotNull = bindUnarySqlValueMethod('isNotNull');
export const runtimeInArray = bindArraySqlValueMethod('inArray');
export const runtimeNotInArray = bindArraySqlValueMethod('notInArray');
export const runtimeBetween = bindTernarySqlValueMethod('between');
export const runtimeNotBetween =
  bindTernarySqlValueMethod('notBetween');
export const runtimeLikeSql = bindBinarySqlValueMethod('likeSql');
export const runtimeNotLikeSql = bindBinarySqlValueMethod('notLikeSql');
export const runtimeExistsSql = bindExistsSqlMethod('existsSql');
export const runtimeNotExistsSql = bindExistsSqlMethod('notExistsSql');
// Runtime builder methods
export const runtimeBuilderClone = bindRuntimeMethod('builderClone');
export const runtimeBuilderDrop = bindRuntimeMethod('builderDrop');
export const runtimeBuilderClear = bindRuntimeMethod('builderClear');
export const runtimeBuilderFromTable =
  bindRuntimeMethod('builderFromTable');
export const runtimeBuilderFromTableAlias = bindRuntimeMethod(
  'builderFromTableAlias',
);
export const runtimeBuilderFromSubquery = bindRuntimeMethod(
  'builderFromSubquery',
);
export const runtimeBuilderDistinct =
  bindRuntimeMethod('builderDistinct');
export const runtimeBuilderDistinctOnColumns = bindRuntimeMethod(
  'builderDistinctOnColumns',
);
export const runtimeBuilderDistinctOnExprs = bindRuntimeMethod(
  'builderDistinctOnExprs',
);
export const runtimeBuilderSelectColumns = bindRuntimeMethod(
  'builderSelectColumns',
);
export const runtimeBuilderSelectAliased = bindRuntimeMethod(
  'builderSelectAliased',
);
export const runtimeBuilderSelectFragment = bindRuntimeMethod(
  'builderSelectFragment',
);
export const runtimeBuilderJoinTable =
  bindRuntimeMethod('builderJoinTable');
export const runtimeBuilderJoinTableAlias = bindRuntimeMethod(
  'builderJoinTableAlias',
);
export const runtimeBuilderJoinSubquery = bindRuntimeMethod(
  'builderJoinSubquery',
);
export const runtimeBuilderOn = bindRuntimeMethod('builderOn');
export const runtimeBuilderAndOn = bindRuntimeMethod('builderAndOn');
export const runtimeBuilderOrOn = bindRuntimeMethod('builderOrOn');
export const runtimeBuilderUsingColumns = bindRuntimeMethod(
  'builderUsingColumns',
);
export const runtimeBuilderOnColumns =
  bindRuntimeMethod('builderOnColumns');
export const runtimeBuilderWhere = bindRuntimeMethod('builderWhere');
export const runtimeBuilderAndWhere =
  bindRuntimeMethod('builderAndWhere');
export const runtimeBuilderOrWhere =
  bindRuntimeMethod('builderOrWhere');
export const runtimeBuilderHaving = bindRuntimeMethod('builderHaving');
export const runtimeBuilderAndHaving =
  bindRuntimeMethod('builderAndHaving');
export const runtimeBuilderOrHaving =
  bindRuntimeMethod('builderOrHaving');
export const runtimeBuilderGroupByColumns = bindRuntimeMethod(
  'builderGroupByColumns',
);
export const runtimeBuilderOrderByColumn = bindRuntimeMethod(
  'builderOrderByColumn',
);
export const runtimeBuilderOrderByColumns = bindRuntimeMethod(
  'builderOrderByColumns',
);
export const runtimeBuilderLimit = bindRuntimeMethod('builderLimit');
export const runtimeBuilderOffset = bindRuntimeMethod('builderOffset');
export const runtimeBuilderForUpdate =
  bindRuntimeMethod('builderForUpdate');
export const runtimeBuilderForShare =
  bindRuntimeMethod('builderForShare');
export const runtimeBuilderNoWait = bindRuntimeMethod('builderNoWait');
export const runtimeBuilderSkipLocked = bindRuntimeMethod(
  'builderSkipLocked',
);
export const runtimeBuilderUnion = bindRuntimeMethod('builderUnion');
export const runtimeBuilderUnionAll =
  bindRuntimeMethod('builderUnionAll');
export const runtimeBuilderIntersect =
  bindRuntimeMethod('builderIntersect');
export const runtimeBuilderExcept = bindRuntimeMethod('builderExcept');
export const runtimeBuilderWith = bindRuntimeMethod('builderWith');
export const runtimeBuilderWithRecursive = bindRuntimeMethod(
  'builderWithRecursive',
);
export const runtimeBuilderInsertInto = bindRuntimeMethod(
  'builderInsertInto',
);
export const runtimeBuilderColumns =
  bindRuntimeMethod('builderColumns');
export const runtimeBuilderValuesInsert = bindRuntimeMethod(
  'builderValuesInsert',
);
export const runtimeBuilderInsertSelect = bindRuntimeMethod(
  'builderInsertSelect',
);
export const runtimeBuilderInsertSelectHandle = bindRuntimeMethod(
  'builderInsertSelectHandle',
);
export const runtimeBuilderOnConflictColumns = bindRuntimeMethod(
  'builderOnConflictColumns',
);
export const runtimeBuilderOnConflictConstraint = bindRuntimeMethod(
  'builderOnConflictConstraint',
);
export const runtimeBuilderDoNothing =
  bindRuntimeMethod('builderDoNothing');
export const runtimeBuilderDoUpdateSet = bindRuntimeMethod(
  'builderDoUpdateSet',
);
export const runtimeBuilderConflictWhere = bindRuntimeMethod(
  'builderConflictWhere',
);
export const runtimeBuilderReturningColumns = bindRuntimeMethod(
  'builderReturningColumns',
);
export const runtimeBuilderReturningAliased = bindRuntimeMethod(
  'builderReturningAliased',
);
export const runtimeBuilderReturningFragment = bindRuntimeMethod(
  'builderReturningFragment',
);
export const runtimeBuilderUpdate = bindRuntimeMethod('builderUpdate');
export const runtimeBuilderSet = bindRuntimeMethod('builderSet');
export const runtimeBuilderDeleteFrom = bindRuntimeMethod(
  'builderDeleteFrom',
);

export function runtimeBuilderGetQuery(handle: string): SqlQuery {
  try {
    return reviveQuery(getBinding().builderQuery(handle));
  } catch (err) {
    rethrowStructuredValidationError(err);
  }
}

export function runtimeBuilderText(handle: string): string {
  try {
    return getBinding().builderText(handle);
  } catch (err) {
    rethrowStructuredValidationError(err);
  }
}

export function runtimeBuilderRaw(handle: string): string {
  try {
    return getBinding().builderRaw(handle);
  } catch (err) {
    rethrowStructuredValidationError(err);
  }
}

export function runtimeBuilderValues(handle: string): Primitive[] {
  try {
    return revivePrimitiveList(getBinding().builderValues(handle));
  } catch (err) {
    rethrowStructuredValidationError(err);
  }
}

export const runtimeBuilderSelectedColumns = bindRuntimeMethod(
  'builderSelectedColumns',
);
export const runtimeBuilderInsertColumns = bindRuntimeMethod(
  'builderInsertColumns',
);

export function runtimeBuilderConflictTargetKind(
  handle: string,
): string {
  const binding = getBinding();
  if (typeof binding.builderConflictTargetKind !== 'function') {
    return 'none';
  }
  return binding.builderConflictTargetKind(handle);
}

export function runtimeBuilderAs(
  handle: string,
  alias: string,
): {
  __kind: 'aliased-query';
  alias: string;
  query: SqlQuery;
  text: string;
  raw: string;
  values: Primitive[];
  __handle?: string;
  selectedColumns: string[];
} {
  const obj = getBinding().builderAs(handle, alias);
  return {
    __kind: 'aliased-query',
    alias: obj.alias,
    query: {
      text: obj.query.text,
      raw: obj.query.raw,
      values: obj.query.values.map(revivePrimitive),
    },
    text: obj.text,
    raw: obj.raw,
    values: obj.values.map(revivePrimitive),
    __handle: obj.__handle,
    selectedColumns: obj.selectedColumns,
  };
}

export function runtimeBuilderCanonicalIrHash(handle: string): string {
  const binding = getBinding();
  if (typeof binding.builderCanonicalIrHash === 'function') {
    return binding.builderCanonicalIrHash(handle);
  }

  // Compatibility path for older native/wasm artifacts that predate
  // builderCanonicalIrHash: derive a deterministic hash from typed query output.
  const query = binding.builderQuery(handle);
  const stablePayload = JSON.stringify({
    text: query.text,
    raw: query.raw,
    values: query.values,
  });
  return `compat-fnv1a64-${hash64Hex(stablePayload)}`;
}

// ─── Handle-passing bridge functions (Phase 5) ────────────────────────────────

export const runtimeBuilderUnionHandle = bindRuntimeMethod(
  'builderUnionHandle',
);
export const runtimeBuilderUnionAllHandle = bindRuntimeMethod(
  'builderUnionAllHandle',
);
export const runtimeBuilderIntersectHandle = bindRuntimeMethod(
  'builderIntersectHandle',
);
export const runtimeBuilderExceptHandle = bindRuntimeMethod(
  'builderExceptHandle',
);
export const runtimeBuilderWithHandle = bindRuntimeMethod(
  'builderWithHandle',
);
export const runtimeBuilderWithRecursiveHandle = bindRuntimeMethod(
  'builderWithRecursiveHandle',
);
export const runtimeBuilderFromSubqueryHandle = bindRuntimeMethod(
  'builderFromSubqueryHandle',
);
export const runtimeBuilderJoinSubqueryHandle = bindRuntimeMethod(
  'builderJoinSubqueryHandle',
);

// ─── Batch ops bridge function (Phase 6) ─────────────────────────────────────

export const runtimeBuilderApplyOps =
  bindRuntimeMethod('builderApplyOps');

export function runtimeCanApplyOpsBinary(): boolean {
  return typeof getBinding().builderApplyOpsBinary === 'function';
}

export function encodePendingOpsBinary(
  ops: readonly unknown[],
): Uint8Array {
  const chunks: Uint8Array[] = [];
  let totalLength = 1 + 4;

  for (const op of ops) {
    const blob = encode(op);
    chunks.push(blob);
    totalLength += 1 + 4 + blob.length;
  }

  const payload = new Uint8Array(totalLength);
  const view = new DataView(
    payload.buffer,
    payload.byteOffset,
    payload.byteLength,
  );
  let offset = 0;

  payload[offset] = 1;
  offset += 1;
  view.setUint32(offset, ops.length, true);
  offset += 4;

  for (const blob of chunks) {
    payload[offset] = 0;
    offset += 1;
    view.setUint32(offset, blob.length, true);
    offset += 4;
    payload.set(blob, offset);
    offset += blob.length;
  }

  return payload;
}

export function runtimeBuilderApplyOpsBinary(
  handle: string,
  payload: Uint8Array,
): string {
  const binding = getBinding();
  if (typeof binding.builderApplyOpsBinary !== 'function') {
    throw new Error(
      'builderApplyOpsBinary is not available in this runtime binding.',
    );
  }
  return binding.builderApplyOpsBinary(handle, payload);
}

export function runtimeBuilderCompileBundle(handle: string): {
  text: string;
  raw: string;
  values: Primitive[];
} {
  const binding = getBinding();
  if (typeof binding.builderCompileBundle !== 'function') {
    return {
      text: binding.builderText(handle),
      raw: binding.builderRaw(handle),
      values: revivePrimitiveList(binding.builderValues(handle)),
    };
  }
  return reviveCompileBundle(binding.builderCompileBundle(handle));
}

export {
  clearWasmBinding,
  setRuntimeBinding,
  setWasmBinding,
} from './get-runtime-binding';
export type {
  RuntimeBinding,
  WireAliasedQuery,
  WireQuery,
} from './types';
export {
  makeDeferredQuery,
  revivePrimitive,
  serializeQuery,
  serializeSqlValue,
};
