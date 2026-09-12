import { isObjectRecord as isRecord } from "../utils/record";
import type { RuntimeBinding } from "./types";

type Primitive = string | number | boolean | bigint | null | Date;
type SqlQuery = {
  text: string;
  raw: string;
  values: Primitive[];
};
type SqlIdentifier = {
  __kind: "identifier";
  parts: string[];
};
type SqlRaw = { __kind: "raw"; text: string };
type SqlValue = SqlQuery | SqlIdentifier | SqlRaw | Primitive | Primitive[];

function isQuery(value: unknown): value is SqlQuery {
  return (
    isRecord(value) && "text" in value && "raw" in value && "values" in value
  );
}

function isIdentifier(value: unknown): value is SqlIdentifier {
  return (
    isRecord(value) &&
    "__kind" in value &&
    (value as { __kind?: string }).__kind === "identifier"
  );
}

function isRaw(value: unknown): value is SqlRaw {
  return (
    isRecord(value) &&
    "__kind" in value &&
    (value as { __kind?: string }).__kind === "raw"
  );
}

function revivePrimitive(value: unknown): Primitive {
  if (isRecord(value) && "__kind" in value) {
    const tagged = value as {
      __kind?: string;
      value?: string;
    };
    if (tagged.__kind === "date" && tagged.value) return new Date(tagged.value);
    if (tagged.__kind === "bigint" && tagged.value) return BigInt(tagged.value);
  }

  return value as Primitive;
}

function renderValue(value: Primitive): string {
  if (value === null) return "NULL";
  if (typeof value === "string") return `'${value.replace(/'/g, "''")}'`;
  if (typeof value === "number" || typeof value === "bigint")
    return String(value);
  if (typeof value === "boolean") return value ? "TRUE" : "FALSE";
  if (value instanceof Date)
    return `'${value.toISOString().replace(/'/g, "''")}'`;
  throw new Error("Unsupported SQL value");
}

function escapeIdentifierPart(part: string): string {
  if (!part.length) throw new Error("Identifier part cannot be empty");
  return `"${part.replace(/"/g, '""')}"`;
}

function renderIdentifierPart(part: string): string {
  if (!part.length) throw new Error("Identifier part cannot be empty");
  return part;
}

function parseValue(json: string): unknown {
  return JSON.parse(json);
}

function toQuery(value: SqlValue): SqlQuery {
  if (isQuery(value)) return value;
  if (isIdentifier(value)) {
    return {
      text: value.parts.map(escapeIdentifierPart).join("."),
      raw: value.parts.map(renderIdentifierPart).join("."),
      values: [],
    };
  }

  if (isRaw(value)) {
    return {
      text: value.text,
      raw: value.text,
      values: [],
    };
  }

  if (Array.isArray(value)) {
    if (value.length === 0)
      throw new Error("Cannot interpolate an empty array");
    return {
      text: value.map(() => "?").join(", "),
      raw: value.map(renderValue).join(", "),
      values: value,
    };
  }

  return {
    text: "?",
    raw: renderValue(value),
    values: [value],
  };
}

export const browserFallbackBinding = {
  identifier(partsJson: string) {
    return JSON.stringify({
      __kind: "identifier",
      parts: parseValue(partsJson),
    });
  },
  raw(text: string) {
    return JSON.stringify({ __kind: "raw", text });
  },
  join(itemsJson: string, separator = ", ") {
    const items = (parseValue(itemsJson) as unknown[]).map(
      (item) => reviveSerialized(item) as SqlValue,
    );
    const textParts: string[] = [];
    const rawParts: string[] = [];
    const values: Primitive[] = [];

    for (const [index, item] of items.entries()) {
      if (index > 0) {
        textParts.push(separator);
        rawParts.push(separator);
      }
      const query = toQuery(item);
      textParts.push(query.text);
      rawParts.push(query.raw);
      values.push(...query.values);
    }

    return JSON.stringify(
      serializeQuery({
        text: textParts.join(""),
        raw: rawParts.join(""),
        values,
      }),
    );
  },
  sql(stringsJson: string, exprsJson: string) {
    const strings = parseValue(stringsJson) as string[];
    const exprs = (parseValue(exprsJson) as unknown[]).map(
      (item) => reviveSerialized(item) as SqlValue,
    );
    const textParts: string[] = [];
    const rawParts: string[] = [];
    const values: Primitive[] = [];

    for (let i = 0; i < strings.length; i++) {
      textParts.push(strings[i] ?? "");
      rawParts.push(strings[i] ?? "");

      if (i >= exprs.length) continue;
      const query = toQuery(exprs[i] as SqlValue);
      textParts.push(query.text);
      rawParts.push(query.raw);
      values.push(...query.values);
    }

    return JSON.stringify(
      serializeQuery({
        text: textParts.join(""),
        raw: rawParts.join(""),
        values,
      }),
    );
  },
  refIdentifier(partsJson: string) {
    return JSON.stringify({
      __kind: "identifier",
      parts: parseValue(partsJson),
    });
  },
  compilePostgres(queryValue: Parameters<RuntimeBinding["compileQuery"]>[0]) {
    return browserFallbackBinding.compileQuery(queryValue, "postgres");
  },
  compileQuery(queryValue: unknown, dialect: string) {
    const query =
      typeof queryValue === "string"
        ? (reviveSerialized(parseValue(queryValue)) as SqlQuery)
        : (reviveSerialized(queryValue) as SqlQuery);
    let placeholderIndex = 0;
    const compiledText = query.text.replace(/\?/g, () => {
      placeholderIndex += 1;

      if (placeholderIndex > query.values.length) return "?";
      if (dialect === "postgres" || dialect === "duckdb")
        return `$${placeholderIndex}`;
      if (dialect === "mssql" || dialect === "mssqlserver")
        return `@p${placeholderIndex}`;
      return "?";
    });

    if (
      ![
        "postgres",
        "duckdb",
        "mysql",
        "sqlite",
        "mssql",
        "mssqlserver",
      ].includes(dialect)
    ) {
      throw new Error(
        `Dialect "${dialect}" does not support SQL placeholder compilation in this builder.`,
      );
    }

    if (placeholderIndex !== query.values.length) {
      throw new Error(
        `Dialect "${dialect}" compilation expected ${query.values.length} placeholders but found ${placeholderIndex}.`,
      );
    }

    return serializeQuery({
      text: compiledText,
      raw: query.raw,
      values: query.values,
    });
  },
  cmp(leftJson: string, operator: string, rightJson: string, dialect: string) {
    const normalized = validateOperatorForDialect(
      operator,
      dialect,
      "comparison",
    );
    const left = reviveSerialized(parseValue(leftJson)) as SqlValue;
    const right = reviveSerialized(parseValue(rightJson)) as SqlValue;
    const leftQuery = toQuery(left);
    const rightQuery = toQuery(right);
    return JSON.stringify(
      serializeQuery({
        text: `${leftQuery.text} ${normalized} ${rightQuery.text}`,
        raw: `${leftQuery.raw} ${normalized} ${rightQuery.raw}`,
        values: [...leftQuery.values, ...rightQuery.values],
      }),
    );
  },
  fnCall(name: string, argsJson: string, dialect: string) {
    const normalized = validateFunctionForDialect(name, dialect);
    const args = (parseValue(argsJson) as unknown[]).map(
      (item) => reviveSerialized(item) as SqlValue,
    );

    if (args.length === 0) {
      return JSON.stringify(
        serializeQuery({
          text: `${normalized}()`,
          raw: `${normalized}()`,
          values: [],
        }),
      );
    }

    const rendered = args.map((arg) => toQuery(arg));
    return JSON.stringify(
      serializeQuery({
        text: `${normalized}(${rendered.map((item) => item.text).join(", ")})`,
        raw: `${normalized}(${rendered.map((item) => item.raw).join(", ")})`,
        values: rendered.flatMap((item) => item.values),
      }),
    );
  },
  arithBinary(
    leftJson: string,
    operator: string,
    rightJson: string,
    dialect: string,
  ) {
    const normalized = validateOperatorForDialect(
      operator,
      dialect,
      "arithmetic",
    );
    const left = reviveSerialized(parseValue(leftJson)) as SqlValue;
    const right = reviveSerialized(parseValue(rightJson)) as SqlValue;
    const leftQuery = toQuery(left);
    const rightQuery = toQuery(right);
    return JSON.stringify(
      serializeQuery({
        text: `(${leftQuery.text} ${normalized} ${rightQuery.text})`,
        raw: `(${leftQuery.raw} ${normalized} ${rightQuery.raw})`,
        values: [...leftQuery.values, ...rightQuery.values],
      }),
    );
  },
  and(conditionsJson: string) {
    return JSON.stringify(
      combineBoolean(parseValue(conditionsJson) as unknown[], "AND"),
    );
  },
  or(conditionsJson: string) {
    return JSON.stringify(
      combineBoolean(parseValue(conditionsJson) as unknown[], "OR"),
    );
  },
} as unknown as RuntimeBinding;

function reviveSerialized(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map((item) => reviveSerialized(item));
  }

  if (isRecord(value)) {
    if (
      "values" in value &&
      Array.isArray((value as { values: unknown[] }).values)
    ) {
      const query = value as {
        text: string;
        raw: string;
        values: unknown[];
      };
      return {
        text: query.text,
        raw: query.raw,
        values: query.values.map(revivePrimitive),
      } satisfies SqlQuery;
    }

    if ("__kind" in value) {
      return value;
    }
  }

  return revivePrimitive(value);
}

function serializePrimitive(value: Primitive): unknown {
  if (typeof value === "bigint")
    return { __kind: "bigint", value: value.toString() };
  if (value instanceof Date)
    return { __kind: "date", value: value.toISOString() };
  return value;
}

function serializeQuery(query: SqlQuery) {
  return {
    text: query.text,
    raw: query.raw,
    values: query.values.map(serializePrimitive),
  };
}

function combineBoolean(conditions: unknown[], operator: "AND" | "OR") {
  if (conditions.length === 0)
    throw new Error(
      `${operator.toLowerCase()}(...) requires at least one condition`,
    );
  const revived = conditions.map((condition) => {
    const value = reviveSerialized(condition) as SqlValue;
    const query =
      typeof value === "string"
        ? toQuery({ __kind: "raw", text: value })
        : toQuery(value);
    return {
      text: `(${query.text})`,
      raw: `(${query.raw})`,
      values: query.values,
    };
  });

  return serializeQuery({
    text: `(${revived.map((item) => item.text).join(` ${operator} `)})`,
    raw: `(${revived.map((item) => item.raw).join(` ${operator} `)})`,
    values: revived.flatMap((item) => item.values),
  });
}

const SQL_DIALECTS = new Set([
  "postgres",
  "duckdb",
  "sqlite",
  "mysql",
  "mssql",
  "mssqlserver",
  "oracle",
  "snowflake",
  "googlesql",
  "redshift",
  "bigquery",
  "clickhouse",
]);

const SUPPORTED_FUNCTIONS = new Set([
  "COUNT",
  "SUM",
  "AVG",
  "MIN",
  "MAX",
  "COALESCE",
  "LOWER",
  "UPPER",
  "LAG",
  "LEAD",
  "ROW_NUMBER",
  "RANK",
  "DENSE_RANK",
]);

const COMPARISON_OPERATORS = new Set(["=", "!=", "<>", ">", ">=", "<", "<="]);
const ARITHMETIC_OPERATORS = new Set(["+", "-", "*", "/"]);

function normalizeFunctionName(name: string): string {
  return name.trim().toUpperCase();
}

function normalizeOperator(operator: string): string {
  return operator.trim();
}

function validateFunctionForDialect(name: string, dialect: string): string {
  const normalized = normalizeFunctionName(name);
  if (!SQL_DIALECTS.has(dialect) || !SUPPORTED_FUNCTIONS.has(normalized)) {
    throw new Error(
      `Dialect "${dialect}" does not support function "${normalized}" in this builder.`,
    );
  }

  return normalized;
}

function validateOperatorForDialect(
  operator: string,
  dialect: string,
  kind: "comparison" | "arithmetic",
): string {
  const normalized = normalizeOperator(operator);
  if (!SQL_DIALECTS.has(dialect)) {
    throw new Error(
      `Dialect "${dialect}" does not support operator "${normalized}" in this builder.`,
    );
  }

  const supported =
    kind === "comparison"
      ? COMPARISON_OPERATORS.has(normalized)
      : ARITHMETIC_OPERATORS.has(normalized);
  if (!supported) {
    throw new Error(
      `Dialect "${dialect}" does not support operator "${normalized}" in this builder.`,
    );
  }

  return normalized;
}

function supportsWindowFunctions(dialect: string): boolean {
  return [
    "postgres",
    "mysql",
    "sqlite",
    "mssql",
    "duckdb",
    "googlesql",
    "oracle",
    "mssqlserver",
    "snowflake",
    "redshift",
    "bigquery",
    "clickhouse",
  ].includes(dialect);
}

type WindowOrderItem = {
  expression: SqlQuery;
  direction?: "ASC" | "DESC";
  nulls?: "FIRST" | "LAST";
};

(
  browserFallbackBinding as unknown as {
    overClause: (
      queryJson: string,
      partitionByJson: string,
      orderByJson: string,
      dialect: string,
    ) => string;
  }
).overClause = (
  queryJson: string,
  partitionByJson: string,
  orderByJson: string,
  dialect: string,
) => {
  if (!supportsWindowFunctions(dialect)) {
    throw new Error(
      `Dialect "${dialect}" does not support window functions in this builder.`,
    );
  }

  const query = reviveSerialized(parseValue(queryJson)) as SqlQuery;
  const partitionBy = (parseValue(partitionByJson) as unknown[]).map(
    (item) => reviveSerialized(item) as SqlQuery,
  );
  const orderBy = (parseValue(orderByJson) as unknown[]).map((item) => {
    const value = item as {
      expression: unknown;
      direction?: "ASC" | "DESC";
      nulls?: "FIRST" | "LAST";
    };
    return {
      expression: reviveSerialized(value.expression) as SqlQuery,
      direction: value.direction,
      nulls: value.nulls,
    } satisfies WindowOrderItem;
  });

  const parts: SqlQuery[] = [];

  if (partitionBy.length > 0) {
    parts.push({
      text: `PARTITION BY ${partitionBy.map((item) => item.text).join(", ")}`,
      raw: `PARTITION BY ${partitionBy.map((item) => item.raw).join(", ")}`,
      values: partitionBy.flatMap((item) => item.values),
    });
  }

  if (orderBy.length > 0) {
    const items = orderBy.map((item) => ({
      text: `${item.expression.text}${item.direction ? ` ${item.direction}` : ""}${item.nulls ? ` NULLS ${item.nulls}` : ""}`,
      raw: `${item.expression.raw}${item.direction ? ` ${item.direction}` : ""}${item.nulls ? ` NULLS ${item.nulls}` : ""}`,
      values: item.expression.values,
    }));
    parts.push({
      text: `ORDER BY ${items.map((item) => item.text).join(", ")}`,
      raw: `ORDER BY ${items.map((item) => item.raw).join(", ")}`,
      values: items.flatMap((item) => item.values),
    });
  }

  return JSON.stringify(
    serializeQuery({
      text: `${query.text} OVER (${parts.map((item) => item.text).join(" ")})`,
      raw: `${query.raw} OVER (${parts.map((item) => item.raw).join(" ")})`,
      values: [...query.values, ...parts.flatMap((item) => item.values)],
    }),
  );
};
