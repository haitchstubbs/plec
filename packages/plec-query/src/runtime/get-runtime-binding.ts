import type { SqlQuery, SqlValue } from '#types';
import { serializeSqlValue } from './bridge';
import { loadNativeBinding } from './native';
import { reviveQuery } from './revivers/revive-query';
import type { RuntimeBinding, WireQuery } from './types';

let runtimeBinding: RuntimeBinding | undefined;
let wasmBinding: RuntimeBinding | undefined;

type RuntimeMethodKey = {
  [K in keyof RuntimeBinding]-?: NonNullable<
    RuntimeBinding[K]
  > extends (...args: infer _Args) => unknown
    ? K
    : never;
}[keyof RuntimeBinding] &
  keyof RuntimeBinding;
type RuntimeMethod<K extends RuntimeMethodKey> =
  NonNullable<RuntimeBinding[K]> extends (
    ...args: infer Args
  ) => infer Return
    ? (...args: Args) => Return
    : never;

export function getBinding(): RuntimeBinding {
  if (wasmBinding) {
    return wasmBinding;
  }

  if (runtimeBinding) {
    return runtimeBinding;
  }

  if (typeof process !== 'undefined' && process.versions?.node) {
    runtimeBinding = loadNativeBinding();
    return runtimeBinding;
  }

  throw new Error(
    '@haitchstack/query runtime binding has not been configured. Use the Node runtime on the server or @haitchstack/query/wasm in the browser.',
  );
}

export function setRuntimeBinding(binding: RuntimeBinding): void {
  runtimeBinding = binding;
}

export function setWasmBinding(binding: RuntimeBinding): void {
  wasmBinding = binding;
}

export function clearWasmBinding(): void {
  wasmBinding = undefined;
}

export function bindRuntimeMethod<K extends RuntimeMethodKey>(
  method: K,
): RuntimeMethod<K> {
  return ((...args: Parameters<RuntimeMethod<K>>) => {
    const fn = getBinding()[method];

    if (typeof fn !== 'function') {
      throw new Error(
        `${String(method)} is not available in this runtime binding.`,
      );
    }

    return (fn as RuntimeMethod<K>)(...args);
  }) as RuntimeMethod<K>;
}

export function bindConditionMethod(
  method: 'and' | 'or',
): (conditions: Array<SqlQuery | string>) => SqlQuery {
  return (conditions) =>
    reviveQuery(
      getBinding()[method](
        conditions.map((condition) =>
          typeof condition === 'string'
            ? { __kind: 'raw', text: condition }
            : serializeSqlValue(condition),
        ),
      ),
    );
}

export function bindUnarySqlValueMethod(
  method: 'isNull' | 'isNotNull',
): (value: SqlValue) => SqlQuery {
  return (value) =>
    reviveQuery(getBinding()[method](serializeSqlValue(value)));
}

export function bindArraySqlValueMethod(
  method: 'inArray' | 'notInArray',
): (value: SqlValue, items: SqlValue[]) => SqlQuery {
  return (value, items) =>
    reviveQuery(
      getBinding()[method](
        serializeSqlValue(value),
        items.map(serializeSqlValue),
      ),
    );
}

export function bindBinarySqlValueMethod(
  method: 'likeSql' | 'notLikeSql',
): (value: SqlValue, pattern: SqlValue) => SqlQuery {
  return (value, pattern) =>
    reviveQuery(
      getBinding()[method](
        serializeSqlValue(value),
        serializeSqlValue(pattern),
      ),
    );
}

export function bindTernarySqlValueMethod(
  method: 'between' | 'notBetween',
): (value: SqlValue, lower: SqlValue, upper: SqlValue) => SqlQuery {
  return (value, lower, upper) =>
    reviveQuery(
      getBinding()[method](
        serializeSqlValue(value),
        serializeSqlValue(lower),
        serializeSqlValue(upper),
      ),
    );
}

export function bindExistsSqlMethod(
  method: 'existsSql' | 'notExistsSql',
): (query: SqlQuery) => SqlQuery {
  return (query) =>
    reviveQuery(
      getBinding()[method](serializeSqlValue(query) as WireQuery),
    );
}
