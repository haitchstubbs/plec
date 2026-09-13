const function_list = [
  'COUNT',
  'SUM',
  'AVG',
  'MIN',
  'MAX',
  'COALESCE',
  'LOWER',
  'UPPER',
  'LAG',
  'LEAD',
  'ROW_NUMBER',
  'RANK',
  'DENSE_RANK',
] as const;

/**
 * Supported SQL aggregate and window function names.
 *
 * @remarks
 * Used when validating function names in query expressions.
 *
 * @example
 * ```ts
 * SUPPORTED_FUNCTIONS.has("SUM");
 * ```
 */
export const SUPPORTED_FUNCTIONS = new Set(function_list);

/**
 * Union type of supported SQL function names.
 *
 * @remarks
 * Matches the values contained in {@link SUPPORTED_FUNCTIONS}.
 *
 * @example
 * ```ts
 * const fnName: SupportedFunction = "COUNT";
 * ```
 */
export type SupportedFunction = (typeof function_list)[number];
