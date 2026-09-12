const operators = ["+", "-", "*", "/"] as const;

/**
 * Supported arithmetic operator tokens.
 *
 * @remarks
 * Used to validate expression operators in query builder logic.
 *
 * @example
 * ```ts
 * ARITHMETIC_OPERATORS.has("+");
 * ```
 */
export const ARITHMETIC_OPERATORS = new Set(operators);
