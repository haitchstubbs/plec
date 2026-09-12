import {
  record,
  string,
  unknown,
  type ZodRecord,
  type ZodString,
  type ZodType,
} from "zod";
import type { $ZodRecordKey, $ZodType, SomeType } from "zod/v4/core";

/**
 * A more specific alias for a Zod record schema with key and value generics.
 *
 * @remarks
 * Keeps the original Zod record type shape while allowing custom key and value types.
 */
type SameZodRecord<
  Key extends $ZodRecordKey = $ZodRecordKey,
  Value extends SomeType = $ZodType,
> = ZodRecord<Key, Value>;

/**
 * Extensions available on Zod record schemas for dynamic field validation.
 */
export interface ZodRecordExtensions<
  Key extends $ZodRecordKey = $ZodRecordKey,
  Value extends SomeType = $ZodType,
> {
  mapDynamicSchema(
    rules: DynamicSchemaRule[],
    options?: DynamicSchemaOptions,
  ): SameZodRecord<Key, Value>;
}

/**
 * Extensions available on Zod string schemas for string pattern helpers.
 */
export interface ZodStringExtensions {
  prefix(prefix: string): this;
  suffix(suffix: string): this;
  startsWithOneOf(prefixes: string[]): this;
  endsWithOneOf(suffixes: string[]): this;
}

/**
 * A rule for matching record keys and assigning a schema to matching values.
 *
 * @remarks
 * Supports prefix, suffix, exact, and custom matcher rules for record validation.
 */
export type DynamicSchemaRule =
  | {
      match: string;
      type: "prefixed";
      schema: ZodType<unknown>;
    }
  | {
      match: string;
      type: "suffixed";
      schema: ZodType<unknown>;
    }
  | {
      match: string;
      type: "exact";
      schema: ZodType<unknown>;
    }
  | {
      match: (key: string) => boolean;
      type?: "custom";
      schema: ZodType<unknown>;
    };

/**
 * Configuration options for dynamic schema validation.
 */
export type DynamicSchemaOptions = {
  unknownKeyMessage?: (key: string) => string;
};

/**
 * Static Zod helpers for query schema validation and string refinements.
 */ // biome-ignore lint/complexity/noStaticOnlyClass: This class is meant to be a static utility class for Zod extensions
export abstract class HaitchstackQueryZodExtension {
  private static dynamicSchemaRuleMatcher(
    rule: DynamicSchemaRule,
    key: string,
  ) {
    if (typeof rule.match === "function") {
      return rule.match(key);
    }

    switch (rule.type) {
      case "prefixed":
        return key.startsWith(rule.match);
      case "suffixed":
        return key.endsWith(rule.match);
      case "exact":
        return key === rule.match;
      default:
        return false;
    }
  }

  /**
   * Creates a Zod record schema that validates each key against the provided rules.
   *
   * @param rules - The list of dynamic schema rules to match record keys.
   * @param options - Optional validation configuration.
   * @returns A Zod record schema that enforces the matched rule schemas.
   */
  static mapDynamicSchema(
    rules: DynamicSchemaRule[],
    options?: DynamicSchemaOptions,
  ) {
    return record(string(), unknown()).superRefine((lake, ctx) => {
      for (const [key, value] of Object.entries(lake)) {
        const rule = rules.find((rule) =>
          HaitchstackQueryZodExtension.dynamicSchemaRuleMatcher(rule, key),
        );

        if (!rule) {
          ctx.addIssue({
            code: "custom",
            path: [key],
            message:
              options?.unknownKeyMessage?.(key) ?? `Unknown dataset "${key}"`,
          });
          continue;
        }

        const result = rule.schema.safeParse(value);

        if (!result.success) {
          for (const issue of result.error.issues) {
            ctx.addIssue({
              code: "custom",
              path: [key, ...issue.path],
              message: issue.message,
            });
          }
        }
      }
    });
  }

  /**
   * Refines a string schema to require one of the provided suffixes.
   *
   * @param stringSchema - The base Zod string schema.
   * @param suffixes - The allowed suffix values.
   * @returns The refined Zod string schema.
   */
  public static stringEndsWithOneOf(
    stringSchema: ZodString,
    suffixes: string[],
  ) {
    return stringSchema.refine(
      (value: string) => suffixes.some((suffix) => value.endsWith(suffix)),
      {
        message: `Value must end with one of: ${suffixes.join(", ")}`,
      },
    ) as ZodString;
  }

  /**
   * Refines a string schema to require one of the provided prefixes.
   *
   * @param stringSchema - The base Zod string schema.
   * @param prefixes - The allowed prefix values.
   * @returns The refined Zod string schema.
   */
  public static stringStartsWithOneOf(
    stringSchema: ZodString,
    prefixes: string[],
  ) {
    return stringSchema.refine(
      (value: string) => prefixes.some((prefix) => value.startsWith(prefix)),
      {
        message: `Value must start with one of: ${prefixes.join(", ")}`,
      },
    ) as ZodString;
  }

  /**
   * Refines a string schema to require a specific suffix.
   *
   * @param stringSchema - The base Zod string schema.
   * @param suffix - The required suffix value.
   * @returns The refined Zod string schema.
   */
  public static stringHasSuffix(stringSchema: ZodString, suffix: string) {
    return stringSchema.refine((value: string) => value.endsWith(suffix), {
      message: `Value must end with "${suffix}"`,
    }) as ZodString;
  }

  /**
   * Refines a string schema to require a specific prefix.
   *
   * @param stringSchema - The base Zod string schema.
   * @param prefix - The required prefix value.
   * @returns The refined Zod string schema.
   */
  public static stringHasPrefix(stringSchema: ZodString, prefix: string) {
    return stringSchema.refine((value: string) => value.startsWith(prefix), {
      message: `Value must start with "${prefix}"`,
    }) as ZodString;
  }
}
