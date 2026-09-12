import { z as zod } from "zod";
import type { $ZodRecordKey, $ZodType, SomeType } from "zod/v4/core";
import {
  HaitchstackQueryZodExtension,
  type ZodRecordExtensions,
  type ZodStringExtensions,
} from "./extended-modules";

/**
 * Extends Zod core schema types with custom string and record helpers.
 *
 * @remarks
 * Adds string prefix/suffix utilities and dynamic record schema mapping to Zod.
 */

declare module "zod" {
  // eslint-disable-next-line @typescript-eslint/no-empty-object-type -- We need to redeclare this to add the method, even if we don't use the type directly
  interface ZodString extends ZodStringExtensions {}
  // eslint-disable-next-line @typescript-eslint/no-empty-object-type -- We need to redeclare this to add the method, even if we don't use the type directly
  interface ZodRecord<
    Key extends $ZodRecordKey = $ZodRecordKey,
    Value extends SomeType = $ZodType,
  > extends ZodRecordExtensions<Key, Value> {}
}

zod.ZodString.prototype.prefix = function (prefix: string) {
  return HaitchstackQueryZodExtension.stringHasPrefix(this, prefix);
};

zod.ZodString.prototype.suffix = function (suffix: string) {
  return HaitchstackQueryZodExtension.stringHasSuffix(this, suffix);
};

zod.ZodString.prototype.startsWithOneOf = function (prefixes: string[]) {
  return HaitchstackQueryZodExtension.stringStartsWithOneOf(this, prefixes);
};

zod.ZodString.prototype.endsWithOneOf = function (suffixes: string[]) {
  return HaitchstackQueryZodExtension.stringEndsWithOneOf(this, suffixes);
};

zod.ZodRecord.prototype.mapDynamicSchema =
  HaitchstackQueryZodExtension.mapDynamicSchema;

/**
 * Re-exports the Zod root object with extended helpers attached.
 *
 * @remarks
 * Use this alias to access Zod along with the custom query schema extensions.
 */
export { zod as z };
