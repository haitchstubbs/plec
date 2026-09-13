import { isObjectRecord as isRecord } from '../utils/record';
import type {
  AliasedQuery,
  AnyAliasedQuery,
  Primitive,
  SqlIdentifier,
  SqlQuery,
  WriteValue,
} from '#types';
import Database from './database';
import { EMPTY_EXPR_VALUES } from './expr';
import type { AnyDatabaseInstance } from './types';

const InternalQueryEngine = {
  makeStaticSqlQuery: (text: string): SqlQuery => {
    return { text, raw: text, values: EMPTY_EXPR_VALUES };
  },
  isAliasedQuery: (value: unknown): value is AnyAliasedQuery => {
    return (
      isRecord(value) &&
      '__kind' in value &&
      (value as { __kind?: string }).__kind === 'aliased-query'
    );
  },
  getAliasedQueryBuilder: (
    value: AnyAliasedQuery,
  ): AnyDatabaseInstance | undefined => {
    const builder = (value as { __builder?: unknown }).__builder;
    return builder instanceof Database
      ? (builder as AnyDatabaseInstance)
      : undefined;
  },
  getAliasedQueryHandle: (
    value: AnyAliasedQuery,
  ): string | undefined => {
    return typeof value.__handle === 'string'
      ? value.__handle
      : undefined;
  },
  createLazyAliasedQuery: <
    TAlias extends string,
    TColumns extends string,
  >(
    builder: AnyDatabaseInstance,
    alias: TAlias,
    materialize: () => AnyAliasedQuery,
  ): AliasedQuery<TAlias, TColumns> => {
    return {
      __kind: 'aliased-query',
      alias,
      get query() {
        return materialize().query;
      },
      get text() {
        return materialize().text;
      },
      get raw() {
        return materialize().raw;
      },
      get values() {
        return materialize().values;
      },
      get __handle() {
        return materialize().__handle;
      },
      get selectedColumns() {
        return materialize().selectedColumns;
      },
      __builder: builder,
    } as AliasedQuery<TAlias, TColumns>;
  },
  toWriteValue(
    value: WriteValue,
  ): Primitive | SqlIdentifier | SqlQuery {
    if (
      isRecord(value) &&
      '__kind' in value &&
      value.__kind === 'value'
    ) {
      return value.value;
    }

    return value;
  },
};

export default InternalQueryEngine;
