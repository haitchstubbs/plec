const operators = ['=', '!=', '<>', '>', '>=', '<', '<='] as const;

/**
 * Supported comparison operator tokens.
 *
 * @remarks
 * Used to validate comparison expressions in query builder logic.
 *
 * @example
 * ```ts
 * COMPARISON_OPERATORS.has("<=");
 * ```
 */
export const COMPARISON_OPERATORS = new Set(operators);
