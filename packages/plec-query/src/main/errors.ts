import type { QueryValidationErrorDetail } from "#types";

/**
 * Thrown by the query builder when the active dialect does not support a
 * feature used in the query.  Inspect `details` for machine-readable per-
 * violation information, or check `feature` on individual entries to react
 * programmatically.
 */
export class QueryValidationError extends Error {
  /** Structured list of individual dialect violations. */
  readonly details: QueryValidationErrorDetail[];

  constructor(details: QueryValidationErrorDetail[]) {
    const summary = details.map((d) => d.message).join("; ");
    super(summary);
    this.name = "QueryValidationError";
    this.details = details;
    // Restore prototype chain in environments that do not support ES2015 class
    // semantics (e.g. transpiled targets).
    Object.setPrototypeOf(this, new.target.prototype);
  }
}
