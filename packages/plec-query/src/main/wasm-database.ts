import type {
  AnyBuilderState,
  AnyDatabaseConnection,
  AnyDatabaseSchema,
  AnySourceColumnMap,
  DatabaseConfig,
  InitialBuilderState,
} from "#types";
import BaseDatabase from "./database";
import { assertNodeQueryWasmInitialized } from "./wasm-runtime";

type EmptySourceColumnMap = Record<never, never>;

class Database<
  TSchema extends AnyDatabaseSchema | undefined = undefined,
  TRegisteredSources extends AnySourceColumnMap = EmptySourceColumnMap,
  TSources extends AnySourceColumnMap = EmptySourceColumnMap,
  TDefaultColumns extends string = never,
  TSelectedColumns extends string = never,
  TState extends AnyBuilderState = InitialBuilderState,
  TConnection extends AnyDatabaseConnection | undefined =
    | AnyDatabaseConnection
    | undefined,
> extends BaseDatabase<
  TSchema,
  TRegisteredSources,
  TSources,
  TDefaultColumns,
  TSelectedColumns,
  TState,
  TConnection
> {
  constructor(config: DatabaseConfig<TSchema, TConnection> = {}) {
    assertNodeQueryWasmInitialized();
    super(config);
  }
}

export { Database };
