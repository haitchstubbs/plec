type BenchmarkableQuery<TResult = unknown> = {
  text: string;
  raw: string;
  values: unknown[] | (() => unknown[]);
  execute?: () => Promise<TResult>;
};

export type QueryPerformanceOptions = {
  execute?: boolean;
  iterations?: number;
  now?: () => number;
};

export type QueryPerformanceResult<TResult = unknown> = {
  iterations: number;
  buildMs: number;
  executeMs: number | null;
  totalMs: number;
  query: {
    text: string;
    raw: string;
    values: unknown[];
  };
  result: TResult | null;
};

export type QueryPerformanceComparison<
  TQueries extends Record<string, () => BenchmarkableQuery<unknown>>,
> = {
  readonly [Name in keyof TQueries]: QueryPerformanceResult;
};

export type QueryLifecyclePerformanceResult<TResult = unknown> = {
  iterations: number;
  chainMs: number;
  materializeMs: number;
  getterReuseMs: number;
  executeMs: number | null;
  totalMs: number;
  query: {
    text: string;
    raw: string;
    values: unknown[];
  };
  result: TResult | null;
};

export type QueryLifecyclePerformanceComparison<
  TQueries extends Record<string, () => BenchmarkableQuery<unknown>>,
> = {
  readonly [Name in keyof TQueries]: QueryLifecyclePerformanceResult;
};

function average(total: number, iterations: number): number {
  return total / iterations;
}

function resolveValues(
  query: BenchmarkableQuery,
  values: BenchmarkableQuery['values'],
): unknown[] {
  return typeof values === 'function' ? values.call(query) : values;
}

function materializeQuery(query: BenchmarkableQuery) {
  return {
    text: query.text,
    raw: query.raw,
    values: resolveValues(query, query.values),
  };
}

export async function measureQueryPerformance<
  TQuery extends BenchmarkableQuery<TResult>,
  TResult = TQuery extends BenchmarkableQuery<infer TResultValue>
    ? TResultValue
    : unknown,
>(
  buildQuery: () => TQuery,
  options: QueryPerformanceOptions = {},
): Promise<QueryPerformanceResult<TResult>> {
  const { execute = false, iterations = 1, now = defaultNow } = options;

  if (!Number.isInteger(iterations) || iterations < 1) {
    throw new Error('iterations must be a positive integer.');
  }

  let buildTotal = 0;
  let executeTotal = 0;
  let lastQuery: TQuery | undefined;
  let lastResult: TResult | null = null;

  for (let index = 0; index < iterations; index++) {
    const buildStart = now();
    const query = buildQuery();
    buildTotal += now() - buildStart;
    lastQuery = query;

    if (execute) {
      if (typeof query.execute !== 'function') {
        throw new Error(
          'Cannot measure execution time for a query without execute().',
        );
      }

      const executeStart = now();
      lastResult = await query.execute();
      executeTotal += now() - executeStart;
    }
  }

  if (!lastQuery) {
    throw new Error('Query builder did not produce a query.');
  }

  return {
    iterations,
    buildMs: average(buildTotal, iterations),
    executeMs: execute ? average(executeTotal, iterations) : null,
    totalMs: average(buildTotal + executeTotal, iterations),
    query: {
      text: lastQuery.text,
      raw: lastQuery.raw,
      values: resolveValues(lastQuery, lastQuery.values),
    },
    result: lastResult,
  };
}

export async function compareQueryPerformance<
  TQueries extends Record<string, () => BenchmarkableQuery<unknown>>,
>(
  queries: TQueries,
  options: QueryPerformanceOptions = {},
): Promise<QueryPerformanceComparison<TQueries>> {
  const entries = await Promise.all(
    Object.entries(queries).map(async ([name, buildQuery]) => [
      name,
      await measureQueryPerformance(buildQuery, options),
    ]),
  );

  return Object.fromEntries(
    entries,
  ) as QueryPerformanceComparison<TQueries>;
}

export async function measureQueryLifecyclePerformance<
  TQuery extends BenchmarkableQuery<TResult>,
  TResult = TQuery extends BenchmarkableQuery<infer TResultValue>
    ? TResultValue
    : unknown,
>(
  buildQuery: () => TQuery,
  options: QueryPerformanceOptions = {},
): Promise<QueryLifecyclePerformanceResult<TResult>> {
  const { execute = false, iterations = 1, now = defaultNow } = options;

  if (!Number.isInteger(iterations) || iterations < 1) {
    throw new Error('iterations must be a positive integer.');
  }

  let chainTotal = 0;
  let materializeTotal = 0;
  let getterReuseTotal = 0;
  let executeTotal = 0;
  let lastMaterialized:
    | {
        text: string;
        raw: string;
        values: unknown[];
      }
    | undefined;
  let lastResult: TResult | null = null;

  for (let index = 0; index < iterations; index++) {
    const chainStart = now();
    const query = buildQuery();
    chainTotal += now() - chainStart;

    const materializeStart = now();
    const materialized = materializeQuery(query);
    materializeTotal += now() - materializeStart;
    lastMaterialized = materialized;

    const getterReuseStart = now();
    void query.text;
    void query.raw;
    resolveValues(query, query.values);
    getterReuseTotal += now() - getterReuseStart;

    if (execute) {
      if (typeof query.execute !== 'function') {
        throw new Error(
          'Cannot measure execution time for a query without execute().',
        );
      }

      const executeStart = now();
      lastResult = await query.execute();
      executeTotal += now() - executeStart;
    }
  }

  if (!lastMaterialized) {
    throw new Error('Query builder did not produce a query.');
  }

  return {
    iterations,
    chainMs: average(chainTotal, iterations),
    materializeMs: average(materializeTotal, iterations),
    getterReuseMs: average(getterReuseTotal, iterations),
    executeMs: execute ? average(executeTotal, iterations) : null,
    totalMs: average(
      chainTotal + materializeTotal + getterReuseTotal + executeTotal,
      iterations,
    ),
    query: lastMaterialized,
    result: lastResult,
  };
}

export async function compareQueryLifecyclePerformance<
  TQueries extends Record<string, () => BenchmarkableQuery<unknown>>,
>(
  queries: TQueries,
  options: QueryPerformanceOptions = {},
): Promise<QueryLifecyclePerformanceComparison<TQueries>> {
  const entries = await Promise.all(
    Object.entries(queries).map(async ([name, buildQuery]) => [
      name,
      await measureQueryLifecyclePerformance(buildQuery, options),
    ]),
  );

  return Object.fromEntries(
    entries,
  ) as QueryLifecyclePerformanceComparison<TQueries>;
}
import { defaultNow } from '../utils/perf';
