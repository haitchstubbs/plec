import * as Core from '#core';
import type * as Types from '#types';

import * as RuntimeBridge from '../runtime/bridge';

import * as Builder from './builder';
import * as Diagnostics from './diagnostics';
import * as Expressions from './expr';
import InternalQueryEngine from './make-query';

type AnyInternalDatabase = Database<
  Types.AnyDatabaseSchema | undefined,
  Types.AnySourceColumnMap,
  Types.AnySourceColumnMap,
  string,
  string,
  Types.AnyBuilderState,
  Types.AnyDatabaseConnection | undefined
>;

type ThisDb<
  TSchema extends Types.AnyDatabaseSchema | undefined,
  TConnection extends Types.AnyDatabaseConnection | undefined,
  TStateNext extends Types.AnyBuilderState,
  TReg extends Types.AnySourceColumnMap,
  TSourcesNext extends Types.AnySourceColumnMap,
  TDefaultNext extends string,
  TSelectedNext extends string,
> = Database<
  TSchema,
  TReg,
  TSourcesNext,
  TDefaultNext,
  TSelectedNext,
  TStateNext,
  TConnection
>;

type Gate<TDb, TGate> = TDb & TGate;

// ─── Builder handle cleanup ───────────────────────────────────────────────────
// When a Database instance is GC'd, drop its Rust builder handle so the
// registry entry is freed promptly instead of waiting for TTL eviction.

class Database<
  TSchema extends Types.AnyDatabaseSchema | undefined = undefined,
  TRegisteredSources extends Types.AnySourceColumnMap =
    Types.EmptySourceColumnMap,
  TSources extends Types.AnySourceColumnMap =
    Types.EmptySourceColumnMap,
  TDefaultColumns extends string = never,
  TSelectedColumns extends string = never,
  TState extends Types.AnyBuilderState = Types.InitialBuilderState,
  TConnection extends Types.AnyDatabaseConnection | undefined =
    Types.AnyDatabaseConnection | undefined,
> {
  /** @internal */
  declare readonly __databaseType?: Types.DatabaseTypeParams<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    TState,
    TConnection
  >;
  /** @internal */
  declare readonly __columns?: TSelectedColumns;

  private static _ephemeral<
    TSchema extends Types.EphemeralGenerics['schema'],
    TConnection extends Types.EphemeralGenerics['connection'],
  >(
    config: Types.DatabaseConfigParams<TSchema, TConnection>,
  ): Types.DatabaseConfigReturnParams<TSchema, TConnection> {
    return new Database<
      TSchema,
      Types.EmptySourceColumnMap,
      Types.EmptySourceColumnMap,
      never,
      never,
      Types.InitialBuilderState,
      TConnection
    >(
      config,
      undefined,
      true,
      Builder.PendingOpBuffer.empty,
      null,
      true,
      true,
    );
  }

  private readonly dialect: Types.Dialect;
  private readonly connection: TConnection | undefined;
  // Raw base Rust handle. May have uncommitted pending ops — use committedHandle
  // for any FFI call that needs the fully applied state.
  private handle: string;
  // Ops buffered in TypeScript, not yet sent to Rust. Applied lazily on
  // output access or before a non-batchable FFI mutation.
  private _pendingOps: Builder.PendingOpBuffer;
  // Strong reference to the parent Database that owns `handle`. Prevents the
  // parent from being GC'd (and its handle from being dropped) while this
  // lazy-buffered instance is still alive.
  private readonly _parent: object | null;
  private readonly _mutable: boolean;
  private readonly _mutableRoot: boolean;
  private readonly _handleCell: Types.BuilderHandleCell;
  // Cached result of commitHandle(). Lazily set on first output access.
  private _materializedHandle: string | undefined;
  // biome-ignore lint/correctness/noUnusedPrivateClassMembers: intentional — same-class private access pattern
  private _materializedHandleCell: Types.BuilderHandleCell | undefined;
  // Cached result of compile(). Set on first call; never shared across instances.
  private _compiledResult: Types.CompiledQuery | undefined;
  private _builderContextCache:
    Types.BuilderContext<string> | undefined;

  constructor(
    config: Types.DatabaseConfig<TSchema, TConnection> = {},
    /** @internal */
    handle?: string,
    /** @internal */
    _ownsHandle = true,
    /** @internal */
    _pendingOps: Builder.PendingOpBuffer = Builder.PendingOpBuffer
      .empty,
    /** @internal */
    _parent: object | null = null,
    /** @internal */
    _mutable = false,
    /** @internal */
    _mutableRoot = false,
  ) {
    this.dialect = config.dialect ?? Core.DEFAULT_DIALECT;
    this.connection = config.connection;
    this._pendingOps = _pendingOps;
    this._parent = _parent;
    this._mutable = _mutable;
    this._mutableRoot = _mutableRoot;
    if (_ownsHandle) {
      this.handle =
        handle ?? RuntimeBridge.runtimeBuilderNew(this.dialect);
    } else {
      if (handle === undefined) {
        throw new Error(
          'Borrowed Database instances require a Rust builder handle.',
        );
      }
      this.handle = handle;
    }
    this._handleCell = { handle: this.handle };
    if (RuntimeBridge.getActiveRuntimeDialect() !== this.dialect)
      RuntimeBridge.setActiveRuntimeDialect(this.dialect);
    if (_ownsHandle) {
      Builder.builderHandleRegistry.register(
        this,
        this._handleCell,
        this,
      );
    }
  }

  private invalidateMaterializedState(): void {
    this._materializedHandle = undefined;
    this._materializedHandleCell = undefined;
    this._compiledResult = undefined;
  }

  private clearPendingAndInvalidate(): void {
    this._pendingOps = Builder.PendingOpBuffer.empty;
    this.invalidateMaterializedState();
  }

  // Returns a Rust handle with all pending ops committed. If there are no
  // pending ops, returns `handle` directly (no FFI call). Otherwise calls
  // builderApplyOps once for all buffered ops and caches the result.
  private commitHandle(): string {
    if (RuntimeBridge.getActiveRuntimeDialect() !== this.dialect)
      RuntimeBridge.setActiveRuntimeDialect(this.dialect);
    void this._parent;
    if (this._mutable) {
      if (this._pendingOps.length === 0) return this.handle;
    } else {
      if (this._materializedHandle !== undefined)
        return this._materializedHandle;
      if (this._pendingOps.length === 0) return this.handle;
    }
    const diagnosticsCollector =
      Diagnostics.DiagnosticsCollector.Compiler();
    if (diagnosticsCollector) {
      diagnosticsCollector.sample.pendingOpCount +=
        this._pendingOps.length;
    }
    let committed: string;
    if (RuntimeBridge.runtimeCanApplyOpsBinary()) {
      try {
        // Binary path: child builders are serialized inline — single FFI call
        // for the entire tree (no per-child runtimeBuilderApplyOpsBinary).
        const payload =
          Diagnostics.QueryDiagnostics.measureDiagnosticsStep(
            diagnosticsCollector,
            'pendingOpsSerializeMs',
            () => {
              const compactOps = this._pendingOps
                .toArray()
                .map((op) => this.serializePendingOpNested(op));
              return RuntimeBridge.encodePendingOpsBinary(compactOps);
            },
          );
        committed = Diagnostics.QueryDiagnostics.measureDiagnosticsStep(
          diagnosticsCollector,
          'pendingOpsApplyMs',
          () =>
            RuntimeBridge.runtimeBuilderApplyOpsBinary(
              this.handle,
              payload,
            ),
        );
        diagnosticsCollector?.recordApplyMode('binary');
      } catch {
        // Fallback: materialize builders to SQL queries, then JSON-encode.
        const resolvedBuilderCache = new Map<
          object,
          {
            handle?: string;
            query?: ReturnType<typeof RuntimeBridge.serializeQuery>;
            selectedColumns?: string[];
          }
        >();
        const pendingOps =
          Diagnostics.QueryDiagnostics.measureDiagnosticsStep(
            diagnosticsCollector,
            'pendingOpsMaterializeMs',
            () =>
              this.resolvePendingOpsForCommit(
                this._pendingOps.toArray(),
                resolvedBuilderCache,
              ),
          );
        const serializedOps = pendingOps.map((op) =>
          Builder.serializePendingOp(op),
        );
        const serializedPayload =
          Diagnostics.QueryDiagnostics.measureDiagnosticsStep(
            diagnosticsCollector,
            'pendingOpsSerializeMs',
            () => JSON.stringify(serializedOps),
          );
        committed = Diagnostics.QueryDiagnostics.measureDiagnosticsStep(
          diagnosticsCollector,
          'pendingOpsApplyMs',
          () =>
            RuntimeBridge.runtimeBuilderApplyOps(
              this.handle,
              serializedPayload,
            ),
        );
        diagnosticsCollector?.recordApplyMode('json');
      }
    } else {
      // JSON path: materialize builders to SQL queries before encoding.
      const resolvedBuilderCache = new Map<
        object,
        {
          handle?: string;
          query?: ReturnType<typeof RuntimeBridge.serializeQuery>;
          selectedColumns?: string[];
        }
      >();
      const pendingOps =
        Diagnostics.QueryDiagnostics.measureDiagnosticsStep(
          diagnosticsCollector,
          'pendingOpsMaterializeMs',
          () =>
            this.resolvePendingOpsForCommit(
              this._pendingOps.toArray(),
              resolvedBuilderCache,
            ),
        );
      const serializedOps = pendingOps.map((op) =>
        Builder.serializePendingOp(op),
      );
      const serializedPayload =
        Diagnostics.QueryDiagnostics.measureDiagnosticsStep(
          diagnosticsCollector,
          'pendingOpsSerializeMs',
          () => JSON.stringify(serializedOps),
        );
      committed = Diagnostics.QueryDiagnostics.measureDiagnosticsStep(
        diagnosticsCollector,
        'pendingOpsApplyMs',
        () =>
          RuntimeBridge.runtimeBuilderApplyOps(
            this.handle,
            serializedPayload,
          ),
      );
      diagnosticsCollector?.recordApplyMode('json');
    }

    if (this._mutable) {
      if (this._handleCell.retired) {
        this._handleCell.retired.push(this.handle);
      } else {
        this._handleCell.retired = [this.handle];
      }
      this.handle = committed;
      this._handleCell.handle = committed;
      this.clearPendingAndInvalidate();
      return committed;
    }

    // Register the materialized handle for cleanup when this instance is GC'd.
    const materializedCell = {
      handle: committed,
    } satisfies Types.BuilderHandleCell;
    Builder.builderHandleRegistry.register(
      this as object,
      materializedCell,
    );
    this._materializedHandle = committed;
    this._materializedHandleCell = materializedCell;
    return committed;
  }

  private resolveNestedBuilderHandle(
    builder: Types.AnyDatabaseInstance,
    resolvedBuilderCache: Map<
      object,
      {
        handle?: string;
        query?: ReturnType<typeof RuntimeBridge.serializeQuery>;
        selectedColumns?: string[];
      }
    >,
  ): string {
    const key = builder as object;
    const cached = resolvedBuilderCache.get(key);
    if (cached?.handle !== undefined) return cached.handle;
    const handle = (builder as unknown as AnyInternalDatabase)
      .committedHandle;
    resolvedBuilderCache.set(key, { ...cached, handle });
    return handle;
  }

  private resolveNestedBuilderQuery(
    builder: Types.AnyDatabaseInstance,
    resolvedBuilderCache: Map<
      object,
      {
        handle?: string;
        query?: ReturnType<typeof RuntimeBridge.serializeQuery>;
        selectedColumns?: string[];
      }
    >,
  ): ReturnType<typeof RuntimeBridge.serializeQuery> {
    const key = builder as object;
    const cached = resolvedBuilderCache.get(key);
    if (cached?.query !== undefined) return cached.query;
    const query = RuntimeBridge.serializeQuery(
      (builder as unknown as AnyInternalDatabase)
        .query as unknown as Types.SqlQuery,
    );
    resolvedBuilderCache.set(key, { ...cached, query });
    return query;
  }

  private resolveNestedBuilderSelectedColumns(
    builder: Types.AnyDatabaseInstance,
    resolvedBuilderCache: Map<
      object,
      {
        handle?: string;
        query?: ReturnType<typeof RuntimeBridge.serializeQuery>;
        selectedColumns?: string[];
      }
    >,
  ): string[] {
    const key = builder as object;
    const cached = resolvedBuilderCache.get(key);
    if (cached?.selectedColumns !== undefined)
      return cached.selectedColumns;
    const selectedColumns = RuntimeBridge.runtimeBuilderSelectedColumns(
      this.resolveNestedBuilderHandle(builder, resolvedBuilderCache),
    );
    resolvedBuilderCache.set(key, { ...cached, selectedColumns });
    return selectedColumns;
  }

  private resolvePendingOpsForCommit(
    pendingOps: readonly Types.PendingOp[],
    resolvedBuilderCache: Map<
      object,
      {
        handle?: string;
        query?: ReturnType<typeof RuntimeBridge.serializeQuery>;
        selectedColumns?: string[];
      }
    >,
  ): readonly Types.PendingOp[] {
    return pendingOps.map((op) => {
      switch (op.op) {
        case 'withBuilder':
          return {
            op: 'withQuery',
            name: op.name,
            query: this.resolveNestedBuilderQuery(
              op.rhsBuilder,
              resolvedBuilderCache,
            ),
          } satisfies Types.PendingOp;
        case 'withRecursiveBuilder':
          return {
            op: 'withRecursiveQuery',
            name: op.name,
            query: this.resolveNestedBuilderQuery(
              op.rhsBuilder,
              resolvedBuilderCache,
            ),
            columns: this.resolveNestedBuilderSelectedColumns(
              op.rhsBuilder,
              resolvedBuilderCache,
            ),
          } satisfies Types.PendingOp;
        case 'fromSubqueryBuilder':
          return {
            op: 'fromSubquery',
            alias: op.alias,
            query: this.resolveNestedBuilderQuery(
              op.rhsBuilder,
              resolvedBuilderCache,
            ),
          } satisfies Types.PendingOp;
        case 'joinSubqueryBuilder':
          return {
            op: 'joinSubquery',
            joinType: op.joinType,
            alias: op.alias,
            query: this.resolveNestedBuilderQuery(
              op.rhsBuilder,
              resolvedBuilderCache,
            ),
          } satisfies Types.PendingOp;
        case 'unionBuilder':
          return {
            op: 'union',
            query: this.resolveNestedBuilderQuery(
              op.rhsBuilder,
              resolvedBuilderCache,
            ),
          } satisfies Types.PendingOp;
        case 'unionAllBuilder':
          return {
            op: 'unionAll',
            query: this.resolveNestedBuilderQuery(
              op.rhsBuilder,
              resolvedBuilderCache,
            ),
          } satisfies Types.PendingOp;
        case 'intersectBuilder':
          return {
            op: 'intersect',
            query: this.resolveNestedBuilderQuery(
              op.rhsBuilder,
              resolvedBuilderCache,
            ),
          } satisfies Types.PendingOp;
        case 'exceptBuilder':
          return {
            op: 'except',
            query: this.resolveNestedBuilderQuery(
              op.rhsBuilder,
              resolvedBuilderCache,
            ),
          } satisfies Types.PendingOp;
        default:
          return op;
      }
    });
  }

  /// Serialize a single pending op for the binary path, encoding nested
  /// builder ops as inline child-op arrays instead of pre-materializing them
  /// via FFI.  All other ops delegate to the module-level `Builder.serializePendingOp`.
  private serializePendingOpNested(
    op: Types.PendingOp,
  ): Types.CompactPendingOp {
    switch (op.op) {
      case 'withBuilder': {
        const child = op.rhsBuilder as unknown as AnyInternalDatabase;
        const committed =
          child._materializedHandle ??
          (child._pendingOps.length === 0 ? child.handle : undefined);
        if (committed !== undefined) {
          return ['wh', op.name, committed];
        }
        const childOps = child._pendingOps
          .toArray()
          .map((cop) => child.serializePendingOpNested(cop));
        return ['whi', op.name, child.handle, childOps];
      }
      case 'withRecursiveBuilder': {
        const child = op.rhsBuilder as unknown as AnyInternalDatabase;
        const committed =
          child._materializedHandle ??
          (child._pendingOps.length === 0 ? child.handle : undefined);
        if (committed !== undefined) {
          return ['wrh', op.name, committed];
        }
        const childOps = child._pendingOps
          .toArray()
          .map((cop) => child.serializePendingOpNested(cop));
        return ['wrhi', op.name, child.handle, childOps];
      }
      case 'fromSubqueryBuilder': {
        const child = op.rhsBuilder as unknown as AnyInternalDatabase;
        const committed =
          child._materializedHandle ??
          (child._pendingOps.length === 0 ? child.handle : undefined);
        if (committed !== undefined) {
          return ['fsqh', op.alias, committed];
        }
        const childOps = child._pendingOps
          .toArray()
          .map((cop) => child.serializePendingOpNested(cop));
        return ['fsqi', op.alias, child.handle, childOps];
      }
      case 'joinSubqueryBuilder': {
        const child = op.rhsBuilder as unknown as AnyInternalDatabase;
        const committed =
          child._materializedHandle ??
          (child._pendingOps.length === 0 ? child.handle : undefined);
        if (committed !== undefined) {
          return ['jsqh', op.joinType, op.alias, committed];
        }
        const childOps = child._pendingOps
          .toArray()
          .map((cop) => child.serializePendingOpNested(cop));
        return ['jsqi', op.joinType, op.alias, child.handle, childOps];
      }
      case 'unionBuilder': {
        const child = op.rhsBuilder as unknown as AnyInternalDatabase;
        const committed =
          child._materializedHandle ??
          (child._pendingOps.length === 0 ? child.handle : undefined);
        if (committed !== undefined) {
          return ['unh', committed];
        }
        const childOps = child._pendingOps
          .toArray()
          .map((cop) => child.serializePendingOpNested(cop));
        return ['uni', child.handle, childOps];
      }
      case 'unionAllBuilder': {
        const child = op.rhsBuilder as unknown as AnyInternalDatabase;
        const committed =
          child._materializedHandle ??
          (child._pendingOps.length === 0 ? child.handle : undefined);
        if (committed !== undefined) {
          return ['uah', committed];
        }
        const childOps = child._pendingOps
          .toArray()
          .map((cop) => child.serializePendingOpNested(cop));
        return ['uai', child.handle, childOps];
      }
      case 'intersectBuilder': {
        const child = op.rhsBuilder as unknown as AnyInternalDatabase;
        const committed =
          child._materializedHandle ??
          (child._pendingOps.length === 0 ? child.handle : undefined);
        if (committed !== undefined) {
          return ['ixh', committed];
        }
        const childOps = child._pendingOps
          .toArray()
          .map((cop) => child.serializePendingOpNested(cop));
        return ['ixi', child.handle, childOps];
      }
      case 'exceptBuilder': {
        const child = op.rhsBuilder as unknown as AnyInternalDatabase;
        const committed =
          child._materializedHandle ??
          (child._pendingOps.length === 0 ? child.handle : undefined);
        if (committed !== undefined) {
          return ['exh', committed];
        }
        const childOps = child._pendingOps
          .toArray()
          .map((cop) => child.serializePendingOpNested(cop));
        return ['exi', child.handle, childOps];
      }
      default:
        return Builder.serializePendingOp(op);
    }
  }

  // Shorthand accessor — use this everywhere an FFI call needs the handle.
  private get committedHandle(): string {
    return this.commitHandle();
  }

  private cloneWithOwnedHandle<
    TNextRegisteredSources extends Types.AnySourceColumnMap,
    TNextSources extends Types.AnySourceColumnMap,
    TNextDefaultColumns extends string,
    TNextSelectedColumns extends string,
    TNextState extends Types.AnyBuilderState,
  >(
    handle: string,
  ): Database<
    TSchema,
    TNextRegisteredSources,
    TNextSources,
    TNextDefaultColumns,
    TNextSelectedColumns,
    TNextState,
    TConnection
  > {
    return Diagnostics.QueryDiagnostics.measureBuildDiagnosticsStep(
      Diagnostics.DiagnosticsCollector.Builder(),
      'cloneWithOwnedHandleMs',
      () => {
        if (this._mutable) {
          if (this._mutableRoot) {
            const cloned = new Database<
              TSchema,
              TNextRegisteredSources,
              TNextSources,
              TNextDefaultColumns,
              TNextSelectedColumns,
              TNextState,
              TConnection
            >(
              {
                dialect: this.dialect,
                connection: this.connection,
              } as Types.DatabaseConfig<TSchema, TConnection>,
              handle,
              true,
              Builder.PendingOpBuffer.empty,
              null,
              true,
              false,
            );
            cloned._builderContextCache = this._builderContextCache;
            return cloned;
          }
          if (this._handleCell.retired) {
            this._handleCell.retired.push(this.handle);
          } else {
            this._handleCell.retired = [this.handle];
          }
          this.handle = handle;
          this._handleCell.handle = handle;
          this._compiledResult = undefined;
          return this as unknown as Database<
            TSchema,
            TNextRegisteredSources,
            TNextSources,
            TNextDefaultColumns,
            TNextSelectedColumns,
            TNextState,
            TConnection
          >;
        }
        const cloned = new Database<
          TSchema,
          TNextRegisteredSources,
          TNextSources,
          TNextDefaultColumns,
          TNextSelectedColumns,
          TNextState,
          TConnection
        >(
          {
            dialect: this.dialect,
            connection: this.connection,
          } as Types.DatabaseConfig<TSchema, TConnection>,
          handle,
          true,
        );
        cloned._builderContextCache = this._builderContextCache;
        return cloned;
      },
    );
  }

  private applyDirectMutation<
    TNextRegisteredSources extends Types.AnySourceColumnMap,
    TNextSources extends Types.AnySourceColumnMap,
    TNextDefaultColumns extends string,
    TNextSelectedColumns extends string,
    TNextState extends Types.AnyBuilderState,
  >(
    mutate: (handle: string) => string,
  ): Database<
    TSchema,
    TNextRegisteredSources,
    TNextSources,
    TNextDefaultColumns,
    TNextSelectedColumns,
    TNextState,
    TConnection
  > {
    const collector = Diagnostics.DiagnosticsCollector.Builder();
    Diagnostics.QueryDiagnostics.incrementBuildCounter(
      collector,
      'directMutationCount',
    );
    return Diagnostics.QueryDiagnostics.measureBuildDiagnosticsStep(
      collector,
      'directMutationMs',
      () => {
        if (RuntimeBridge.getActiveRuntimeDialect() !== this.dialect)
          RuntimeBridge.setActiveRuntimeDialect(this.dialect);
        const baseHandle =
          this._pendingOps.length === 0
            ? this.handle
            : this.committedHandle;
        Diagnostics.QueryDiagnostics.incrementBuildCounter(
          collector,
          'runtimeCallCount',
        );
        const nextHandle =
          Diagnostics.QueryDiagnostics.measureBuildDiagnosticsStep(
            collector,
            'runtimeCallMs',
            () => mutate(baseHandle),
          );
        if (this._mutable) {
          if (baseHandle === this.handle) {
            if (this._handleCell.retired) {
              this._handleCell.retired.push(this.handle);
            } else {
              this._handleCell.retired = [this.handle];
            }
          }
          this.handle = nextHandle;
          this._handleCell.handle = nextHandle;
          this.clearPendingAndInvalidate();
          return this as unknown as Database<
            TSchema,
            TNextRegisteredSources,
            TNextSources,
            TNextDefaultColumns,
            TNextSelectedColumns,
            TNextState,
            TConnection
          >;
        }
        return this.cloneWithOwnedHandle<
          TNextRegisteredSources,
          TNextSources,
          TNextDefaultColumns,
          TNextSelectedColumns,
          TNextState
        >(nextHandle);
      },
    );
  }

  // Produce a new Database that buffers one more op without crossing FFI.
  // The new instance BORROWS `this.handle` (or the already-materialized handle)
  // and holds a reference to `this` to prevent premature GC of the base handle.
  private cloneWithPendingOp<
    TNextRegisteredSources extends Types.AnySourceColumnMap,
    TNextSources extends Types.AnySourceColumnMap,
    TNextDefaultColumns extends string,
    TNextSelectedColumns extends string,
    TNextState extends Types.AnyBuilderState,
  >(
    op: Types.PendingOp,
  ): Database<
    TSchema,
    TNextRegisteredSources,
    TNextSources,
    TNextDefaultColumns,
    TNextSelectedColumns,
    TNextState,
    TConnection
  > {
    if (this._mutable) {
      if (this._mutableRoot) {
        const cloned = new Database<
          TSchema,
          TNextRegisteredSources,
          TNextSources,
          TNextDefaultColumns,
          TNextSelectedColumns,
          TNextState,
          TConnection
        >(
          {
            dialect: this.dialect,
            connection: this.connection,
          } as Types.DatabaseConfig<TSchema, TConnection>,
          this.handle,
          false,
          this._pendingOps.append(op),
          this,
          true,
          false,
        );
        cloned._builderContextCache = this._builderContextCache;
        return cloned;
      }
      this._pendingOps = this._pendingOps.append(op);
      this.invalidateMaterializedState();
      return this as unknown as Database<
        TSchema,
        TNextRegisteredSources,
        TNextSources,
        TNextDefaultColumns,
        TNextSelectedColumns,
        TNextState,
        TConnection
      >;
    }
    const cloned = new Database<
      TSchema,
      TNextRegisteredSources,
      TNextSources,
      TNextDefaultColumns,
      TNextSelectedColumns,
      TNextState,
      TConnection
    >(
      {
        dialect: this.dialect,
        connection: this.connection,
      } as Types.DatabaseConfig<TSchema, TConnection>,
      this.handle, // borrow — parent owns this handle
      false, // does NOT own the handle
      this._pendingOps.append(op), // extend the buffered op list
      this, // prevent this (and its handle) from being GC'd
    );
    cloned._builderContextCache = this._builderContextCache;
    return cloned;
  }

  private resolveSelectInput<
    TColumns extends Types.SelectInput<TSources, TDefaultColumns>,
  >(
    input:
      | TColumns
      | ((
          db: Types.BuilderContext<
            Types.SelectedColumn<TSources, TDefaultColumns>
          >,
        ) => TColumns),
  ): TColumns {
    if (typeof input === 'function') {
      return Diagnostics.QueryDiagnostics.measureBuildDiagnosticsStep(
        Diagnostics.DiagnosticsCollector.Builder(),
        'callbackResolveMs',
        () =>
          input(
            this.createBuilderContext<
              Types.SelectedColumn<TSources, TDefaultColumns>
            >(),
          ),
      );
    }

    return input;
  }

  private resolveDistinctOnInput(
    input:
      | Types.DistinctOnInput<TSources, TDefaultColumns>
      | ((
          db: Types.BuilderContext<
            Types.SelectedColumn<TSources, TDefaultColumns>
          >,
        ) => Types.DistinctOnInput<TSources, TDefaultColumns>),
  ): Types.DistinctOnInput<TSources, TDefaultColumns> {
    if (typeof input === 'function') {
      return Diagnostics.QueryDiagnostics.measureBuildDiagnosticsStep(
        Diagnostics.DiagnosticsCollector.Builder(),
        'callbackResolveMs',
        () =>
          input(
            this.createBuilderContext<
              Types.SelectedColumn<TSources, TDefaultColumns>
            >(),
          ),
      );
    }

    return input;
  }

  private serializeDistinctOnValue(
    value: Types.DistinctOnInput<TSources, TDefaultColumns>[number],
  ): Record<string, unknown> {
    if (typeof value === 'string') {
      return { type: 'ref', parts: value.split('.') };
    }

    if (value === null) {
      return { type: 'val', value: null };
    }

    if (typeof value === 'boolean' || typeof value === 'number') {
      return { type: 'val', value };
    }

    if (typeof value === 'bigint' || value instanceof Date) {
      return {
        type: 'val',
        value: RuntimeBridge.serializeSqlValue(value as Types.SqlValue),
      };
    }

    const obj = value as Record<string, unknown>;

    if (obj.__kind === 'value') {
      return {
        type: 'val',
        value: RuntimeBridge.serializeSqlValue(
          (value as Types.ValueLiteral<Types.Primitive>)
            .value as Types.SqlValue,
        ),
      };
    }

    if (obj.__kind === 'identifier') {
      return {
        type: 'ref',
        parts: (value as Types.SqlIdentifier).parts,
      };
    }

    if (obj.__kind === 'raw') {
      return { type: 'raw', text: obj.text as string };
    }

    if ('type' in obj && typeof obj.type === 'string') {
      return obj;
    }

    if ('text' in obj && 'raw' in obj && 'values' in obj) {
      const query = value as Types.SqlQuery;
      return {
        type: 'query',
        query: {
          text: query.text,
          raw: query.raw,
          values: query.values.map((item) =>
            RuntimeBridge.serializeSqlValue(item as Types.SqlValue),
          ),
        },
      };
    }

    return {
      type: 'val',
      value: RuntimeBridge.serializeSqlValue(value as Types.SqlValue),
    };
  }

  private applyDistinctOnInput(
    resolvedColumns: Types.DistinctOnInput<TSources, TDefaultColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithDistinct<TState>,
    TConnection
  > {
    if (resolvedColumns.length === 0) {
      throw new Error(
        'distinctOn(...) requires at least one expression.',
      );
    }

    const allColumns = resolvedColumns.every(
      (column) => typeof column === 'string',
    );
    if (allColumns) {
      return this.cloneWithPendingOp<
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        Types.WithDistinct<TState>
      >({
        op: 'distinctOnColumns',
        cols: resolvedColumns as string[],
      });
    }

    const exprs = resolvedColumns.map((column) =>
      this.serializeDistinctOnValue(column),
    );
    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithDistinct<TState>
    >({
      op: 'distinctOnExprs',
      exprs,
    });
  }

  private resolvePredicateInput<TAvailable extends string>(
    input:
      | Types.PredicateInput
      | ((
          db: Types.BuilderContext<TAvailable>,
        ) => Types.PredicateInput),
  ): Types.PredicateInput {
    if (typeof input === 'function') {
      return Diagnostics.QueryDiagnostics.measureBuildDiagnosticsStep(
        Diagnostics.DiagnosticsCollector.Builder(),
        'callbackResolveMs',
        () => input(this.createBuilderContext<TAvailable>()),
      );
    }

    return input;
  }

  private resolveQueryOperandColumns(
    query: Types.AnyQueryOperand,
  ): string[] | undefined {
    const cols =
      query instanceof Database
        ? RuntimeBridge.runtimeBuilderSelectedColumns(
            query.committedHandle,
          )
        : (query.selectedColumns ?? []);
    return cols.length > 0 ? cols : undefined;
  }

  private applyProjectionInput<
    TColumns extends Types.SelectInput<TSources, TDefaultColumns>,
    TNextSelectedColumns extends string,
    TNextState extends Types.AnyBuilderState,
  >(
    resolvedColumns: TColumns,
    ops: {
      aliased: 'selectAliased' | 'returningAliased';
      columns: 'selectColumns' | 'returningColumns';
    },
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TNextSelectedColumns,
    TNextState,
    TConnection
  > {
    const applyAliased = (entries: unknown[]) =>
      this.cloneWithPendingOp<
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TNextSelectedColumns,
        TNextState
      >({
        op: ops.aliased,
        entries: entries as Types.PendingAliasedEntry[],
      });

    const applyColumns = (cols: string[]) =>
      this.cloneWithPendingOp<
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TNextSelectedColumns,
        TNextState
      >({
        op: ops.columns,
        cols,
      });

    if (Array.isArray(resolvedColumns)) {
      const hasExpressions = resolvedColumns.some(
        (column) => typeof column !== 'string',
      );
      if (hasExpressions) {
        const allExprNodeBacked = resolvedColumns
          .filter((c) => typeof c !== 'string')
          .every(
            (c) =>
              (c as unknown as { __exprNode?: unknown }).__exprNode !==
              undefined,
          );

        if (allExprNodeBacked) {
          const entries = resolvedColumns.map((column) => {
            if (typeof column === 'string') {
              const parts = column.split('.');
              const alias = parts.at(-1) ?? column;
              return {
                alias,
                bare: true,
                expr: {
                  type: 'ref',
                  parts,
                  text: '',
                  raw: '',
                  values: Expressions.EMPTY_EXPR_VALUES,
                },
              };
            }
            return {
              alias: column.alias,
              expr: (column as unknown as { __exprNode: unknown })
                .__exprNode,
            };
          });
          return applyAliased(entries);
        }

        const entries = resolvedColumns.map((column) => {
          if (typeof column === 'string') {
            const parts = column.split('.');
            const alias = parts.at(-1) ?? column;
            return {
              alias,
              bare: true,
              expr: {
                type: 'ref',
                parts,
                text: '',
                raw: '',
                values: Expressions.EMPTY_EXPR_VALUES,
              },
            };
          }
          const exprNode = (
            column as unknown as { __exprNode?: unknown }
          ).__exprNode;
          if (exprNode !== undefined) {
            return {
              alias: column.alias,
              expr: exprNode,
            };
          }
          return {
            alias: column.alias,
            expr: {
              type: 'query',
              query: RuntimeBridge.serializeSqlValue(column.query),
              text: '',
              raw: '',
              values: Expressions.EMPTY_EXPR_VALUES,
            },
          };
        });
        return applyAliased(entries);
      }

      return applyColumns(resolvedColumns as string[]);
    }

    const resolvedColumnObject = resolvedColumns as Record<
      string,
      unknown
    >;

    const fastExprEntries: Array<{ alias: string; expr: unknown }> = [];
    for (const alias in resolvedColumnObject) {
      if (!Object.hasOwn(resolvedColumnObject, alias)) continue;
      const col = resolvedColumnObject[alias];
      if (typeof col === 'string') {
        fastExprEntries.push({
          alias,
          expr: {
            type: 'ref',
            parts: col.split('.'),
            text: '',
            raw: '',
            values: Expressions.EMPTY_EXPR_VALUES,
          },
        });
        continue;
      }

      const obj = col as Record<string, unknown>;
      if ('__exprNode' in obj && obj.__exprNode !== undefined) {
        fastExprEntries.push({ alias, expr: obj.__exprNode });
        continue;
      }

      if ('type' in obj && typeof obj.type === 'string') {
        fastExprEntries.push({ alias, expr: col });
        continue;
      }

      fastExprEntries.length = 0;
      break;
    }

    if (fastExprEntries.length > 0) {
      return applyAliased(fastExprEntries);
    }

    const objectEntries = Object.entries(resolvedColumnObject);

    const hasExprNodeBacked = objectEntries.some(([, col]) => {
      if (typeof col === 'string') return false;
      const obj = col as Record<string, unknown>;
      return (
        ('__exprNode' in obj && obj.__exprNode !== undefined) ||
        ('type' in obj && typeof obj.type === 'string')
      );
    });

    if (hasExprNodeBacked) {
      const entries = objectEntries.map(([alias, col]) => {
        if (typeof col === 'string') {
          const parts = col.split('.');
          return {
            alias,
            expr: {
              type: 'ref',
              parts,
              text: '',
              raw: '',
              values: Expressions.EMPTY_EXPR_VALUES,
            },
          };
        }
        const obj = col as Record<string, unknown>;
        if ('__exprNode' in obj && obj.__exprNode !== undefined) {
          return { alias, expr: obj.__exprNode };
        }
        if ('type' in obj && typeof obj.type === 'string') {
          return { alias, expr: col };
        }
        const q = col as Types.SqlQuery;
        return {
          alias,
          expr: {
            type: 'query',
            query: {
              text: q.text,
              raw: q.raw,
              values: q.values.map((v) =>
                RuntimeBridge.serializeSqlValue(v as Types.SqlValue),
              ),
            },
            text: '',
            raw: '',
            values: Expressions.EMPTY_EXPR_VALUES,
          },
        };
      });
      return applyAliased(entries);
    }

    const entries = objectEntries.map(([alias, col]) => {
      if (typeof col === 'string') {
        const parts = col.split('.');
        return {
          alias,
          expr: {
            type: 'ref',
            parts,
            text: '',
            raw: '',
            values: Expressions.EMPTY_EXPR_VALUES,
          },
        };
      }
      return {
        alias,
        expr: {
          type: 'query',
          query: RuntimeBridge.serializeSqlValue(col as Types.SqlValue),
          text: '',
          raw: '',
          values: Expressions.EMPTY_EXPR_VALUES,
        },
      };
    });
    return applyAliased(entries);
  }

  private normalizeInsertRows<TColumn extends string>(
    rows: Types.InsertRow<TColumn> | Array<Types.InsertRow<TColumn>>,
    explicitColumns: string[],
  ): { columnNames: string[]; rows: unknown[][] } {
    const inputRows = Array.isArray(rows) ? rows : [rows];

    if (inputRows.length === 0) {
      throw new Error('values(...) requires at least one row.');
    }

    const firstRow = inputRows[0] ?? {};
    const inferredColumns =
      explicitColumns.length > 0
        ? explicitColumns
        : Object.keys(firstRow);

    if (inferredColumns.length === 0) {
      throw new Error('values(...) requires at least one column.');
    }

    const serializedRows = inputRows.map((row) => {
      const keys = Object.keys(row);

      if (
        keys.length !== inferredColumns.length ||
        !inferredColumns.every((column) => keys.includes(column))
      ) {
        throw new Error(
          'All values(...) rows must share the same column keys.',
        );
      }

      return inferredColumns.map((column) => {
        if (!(column in row)) {
          throw new Error(
            `Missing value for insert column "${column}".`,
          );
        }

        return RuntimeBridge.serializeSqlValue(
          InternalQueryEngine.toWriteValue(
            row[column as keyof typeof row] as Types.WriteValue,
          ) as Types.SqlValue,
        );
      });
    });

    return { columnNames: inferredColumns, rows: serializedRows };
  }

  private createBuilderContext<
    TAvailable extends string,
  >(): Types.BuilderContext<TAvailable> {
    if (this._builderContextCache !== undefined) {
      Diagnostics.QueryDiagnostics.incrementBuildCounter(
        Diagnostics.DiagnosticsCollector.Builder(),
        'builderContextCacheHits',
      );
      return this
        ._builderContextCache as unknown as Types.BuilderContext<TAvailable>;
    }

    const collector = Diagnostics.DiagnosticsCollector.Builder();
    const startedAt = collector?.now();
    const dialect = this.dialect;
    const sharedContext =
      Builder.sharedBuilderContextCache.get(dialect);
    if (sharedContext !== undefined) {
      this._builderContextCache = sharedContext;
      Diagnostics.QueryDiagnostics.incrementBuildCounter(
        collector,
        'builderContextCacheHits',
      );
      return sharedContext as unknown as Types.BuilderContext<TAvailable>;
    }
    Diagnostics.QueryDiagnostics.incrementBuildCounter(
      collector,
      'builderContextCacheMisses',
    );

    // ── ExprNode factories ──────────────────────────────────────────────────
    // ExprNode objects are plain JSON-serializable records that also satisfy
    // the Types.SqlQuery structural type (via the dummy text/raw/values fields).
    // Rust's Predicate/ExprNode deserializer picks them up by the `"type"`
    // discriminant and evaluates the tree — zero FFI until commitHandle().

    // ── ExprNode-backed FunctionExpression ──────────────────────────────────
    // Wraps an ExprNode inside an object that satisfies the FunctionExpression
    // structural type (SqlQuery + __kind + as() + over()).  No Rust FFI here.

    function createExprFunctionExpression(
      node: Types.SqlQuery,
    ): Types.FunctionExpression {
      function makeOver(
        build?: (b: Types.WindowBuilder) => Types.WindowBuilder,
      ): Types.FunctionExpression {
        const partitionBy: Types.SqlQuery[] = [];
        const orderBy: Array<{
          expression: Types.SqlQuery;
          direction?: string;
          nulls?: string;
        }> = [];

        const wb: Types.WindowBuilder = {
          partitionBy(...expressions) {
            for (const expr of expressions) {
              partitionBy.push(
                Expressions.toExprOperand(
                  expr as Types.SelectExpressionInput<TAvailable>,
                ),
              );
            }
            return wb;
          },
          orderBy(expression, direction?, nulls?) {
            orderBy.push({
              expression: Expressions.toExprOperand(
                expression as Types.SelectExpressionInput<TAvailable>,
              ),
              direction: direction?.toUpperCase(),
              nulls: nulls?.toUpperCase(),
            });
            return wb;
          },
        };

        if (build) {
          const result = build(wb);
          if (result !== wb) {
            throw new Error(
              'Window builder callbacks must return the provided builder.',
            );
          }
        }

        const overNode = Expressions.makeExprNode({
          type: 'over',
          expr: node,
          partitionBy,
          orderBy,
          dialect,
        });
        return createExprFunctionExpression(overNode);
      }

      const fe = {
        ...(node as object),
        __kind: 'function-expression' as const,
        // __exprNode lets select() fast-path and msgpack encoding detect this as a
        // pure data ExprNode, bypassing the function-property spread on `node`.
        __exprNode: node,
        text: '',
        raw: '',
        values: Expressions.EMPTY_EXPR_VALUES,
        as<TAlias extends string>(
          alias: TAlias,
        ): Types.AliasedSelectExpression<TAlias> {
          return {
            __kind: 'aliased-select-expression' as const,
            alias,
            query: node,
            text: '',
            raw: '',
            values: Expressions.EMPTY_EXPR_VALUES,
            // Extra field — lets select() detect ExprNode-backed columns.
            __exprNode: node,
          } as unknown as Types.AliasedSelectExpression<TAlias>;
        },
        over: makeOver,
      } satisfies Types.FunctionExpression;
      return fe;
    }

    function exprFnCall(
      name: string,
      args: Array<
        | Types.SelectExpressionInput<TAvailable>
        | '*'
        | Types.SqlIdentifier
      >,
    ): Types.FunctionExpression {
      const argNodes = args.map((arg) =>
        Expressions.toExprOperand(arg),
      );
      const fnNode = Expressions.makeExprNode({
        type: 'fnCall',
        name,
        args: argNodes,
      });
      return createExprFunctionExpression(fnNode);
    }

    // ── Types.BuilderContext implementation ───────────────────────────────────────

    const context = {
      col: <TColumn extends TAvailable>(column: TColumn) => column,
      ref: <TColumn extends TAvailable>(column: TColumn) => ({
        __kind: 'identifier' as const,
        parts: column.split('.'),
      }),
      excluded: <TColumn extends string>(column: TColumn) =>
        Expressions.excludedExpr(column),
      val: <TValue extends Types.Primitive>(value: TValue) => ({
        __kind: 'value' as const,
        value,
      }),
      fn: (name, ...args) => exprFnCall(name.toUpperCase(), args),
      agg: (name, ...args) => exprFnCall(name.toUpperCase(), args),
      lag: (value, offset, defaultValue) =>
        exprFnCall(
          'LAG',
          defaultValue === undefined
            ? offset === undefined
              ? [value]
              : [value, offset]
            : [value, offset ?? 1, defaultValue],
        ),
      lead: (value, offset, defaultValue) =>
        exprFnCall(
          'LEAD',
          defaultValue === undefined
            ? offset === undefined
              ? [value]
              : [value, offset]
            : [value, offset ?? 1, defaultValue],
        ),
      rowNumber: () => exprFnCall('ROW_NUMBER', []),
      rank: () => exprFnCall('RANK', []),
      denseRank: () => exprFnCall('DENSE_RANK', []),
      cmp: <TLeft extends TAvailable>(
        left: TLeft,
        operator: Types.ComparisonOperator,
        right:
          | TAvailable
          | Types.NonStringPrimitive
          | Types.ValueLiteral<Types.Primitive>,
      ) =>
        Expressions.makeExprNode({
          type: 'cmp',
          left: Expressions.exprRef(left),
          op: operator,
          right: Expressions.toExprOperand(right),
        }),
      eq: <TLeft extends TAvailable>(
        left: TLeft,
        right:
          | TAvailable
          | Types.NonStringPrimitive
          | Types.ValueLiteral<Types.Primitive>,
      ) =>
        Expressions.makeExprNode({
          type: 'cmp',
          left: Expressions.exprRef(left),
          op: '=',
          right: Expressions.toExprOperand(right),
        }),
      ne: <TLeft extends TAvailable>(
        left: TLeft,
        right:
          | TAvailable
          | Types.NonStringPrimitive
          | Types.ValueLiteral<Types.Primitive>,
      ) =>
        Expressions.makeExprNode({
          type: 'cmp',
          left: Expressions.exprRef(left),
          op: '<>',
          right: Expressions.toExprOperand(right),
        }),
      gt: <TLeft extends TAvailable>(
        left: TLeft,
        right:
          | TAvailable
          | Types.NonStringPrimitive
          | Types.ValueLiteral<Types.Primitive>,
      ) =>
        Expressions.makeExprNode({
          type: 'cmp',
          left: Expressions.exprRef(left),
          op: '>',
          right: Expressions.toExprOperand(right),
        }),
      gte: <TLeft extends TAvailable>(
        left: TLeft,
        right:
          | TAvailable
          | Types.NonStringPrimitive
          | Types.ValueLiteral<Types.Primitive>,
      ) =>
        Expressions.makeExprNode({
          type: 'cmp',
          left: Expressions.exprRef(left),
          op: '>=',
          right: Expressions.toExprOperand(right),
        }),
      lt: <TLeft extends TAvailable>(
        left: TLeft,
        right:
          | TAvailable
          | Types.NonStringPrimitive
          | Types.ValueLiteral<Types.Primitive>,
      ) =>
        Expressions.makeExprNode({
          type: 'cmp',
          left: Expressions.exprRef(left),
          op: '<',
          right: Expressions.toExprOperand(right),
        }),
      lte: <TLeft extends TAvailable>(
        left: TLeft,
        right:
          | TAvailable
          | Types.NonStringPrimitive
          | Types.ValueLiteral<Types.Primitive>,
      ) =>
        Expressions.makeExprNode({
          type: 'cmp',
          left: Expressions.exprRef(left),
          op: '<=',
          right: Expressions.toExprOperand(right),
        }),
      isNull: <TValue extends TAvailable>(value: TValue) =>
        Expressions.makeExprNode({
          type: 'isNull',
          value: Expressions.exprRef(value),
        }),
      isNotNull: <TValue extends TAvailable>(value: TValue) =>
        Expressions.makeExprNode({
          type: 'isNotNull',
          value: Expressions.exprRef(value),
        }),
      inArray: <TValue extends TAvailable>(
        value: TValue,
        items: Types.Primitive[] | Types.SqlQuery,
      ) => {
        if (Array.isArray(items)) {
          if (items.length === 0)
            throw new Error('Cannot interpolate an empty array');
          return Expressions.makeExprNode({
            type: 'inArray',
            value: Expressions.exprRef(value),
            items: items.map((v) =>
              RuntimeBridge.serializeSqlValue(v as Types.SqlValue),
            ),
          });
        }
        return Expressions.makeExprNode({
          type: 'inArray',
          value: Expressions.exprRef(value),
          items: {
            text: items.text,
            raw: items.raw,
            values: items.values.map((v) =>
              RuntimeBridge.serializeSqlValue(v as Types.SqlValue),
            ),
          },
        });
      },
      notInArray: <TValue extends TAvailable>(
        value: TValue,
        items: Types.Primitive[] | Types.SqlQuery,
      ) => {
        if (Array.isArray(items)) {
          if (items.length === 0)
            throw new Error('Cannot interpolate an empty array');
          return Expressions.makeExprNode({
            type: 'notInArray',
            value: Expressions.exprRef(value),
            items: items.map((v) =>
              RuntimeBridge.serializeSqlValue(v as Types.SqlValue),
            ),
          });
        }
        return Expressions.makeExprNode({
          type: 'notInArray',
          value: Expressions.exprRef(value),
          items: {
            text: items.text,
            raw: items.raw,
            values: items.values.map((v) =>
              RuntimeBridge.serializeSqlValue(v as Types.SqlValue),
            ),
          },
        });
      },
      between: <TValue extends TAvailable>(
        value: TValue,
        lower:
          | TAvailable
          | Types.NonStringPrimitive
          | Types.ValueLiteral<Types.Primitive>,
        upper:
          | TAvailable
          | Types.NonStringPrimitive
          | Types.ValueLiteral<Types.Primitive>,
      ) =>
        Expressions.makeExprNode({
          type: 'between',
          value: Expressions.exprRef(value),
          lower: Expressions.toExprOperand(lower),
          upper: Expressions.toExprOperand(upper),
        }),
      notBetween: <TValue extends TAvailable>(
        value: TValue,
        lower:
          | TAvailable
          | Types.NonStringPrimitive
          | Types.ValueLiteral<Types.Primitive>,
        upper:
          | TAvailable
          | Types.NonStringPrimitive
          | Types.ValueLiteral<Types.Primitive>,
      ) =>
        Expressions.makeExprNode({
          type: 'notBetween',
          value: Expressions.exprRef(value),
          lower: Expressions.toExprOperand(lower),
          upper: Expressions.toExprOperand(upper),
        }),
      like: <TValue extends TAvailable>(
        value: TValue,
        pattern: Types.SelectExpressionInput<TAvailable>,
      ) =>
        Expressions.makeExprNode({
          type: 'like',
          value: Expressions.exprRef(value),
          pattern: Expressions.toExprOperand(pattern),
        }),
      notLike: <TValue extends TAvailable>(
        value: TValue,
        pattern: Types.SelectExpressionInput<TAvailable>,
      ) =>
        Expressions.makeExprNode({
          type: 'notLike',
          value: Expressions.exprRef(value),
          pattern: Expressions.toExprOperand(pattern),
        }),
      exists: (query: Types.SqlQuery) =>
        Expressions.makeExprNode({
          type: 'exists',
          query: {
            text: query.text,
            raw: query.raw,
            values: query.values.map((v) =>
              RuntimeBridge.serializeSqlValue(v as Types.SqlValue),
            ),
          },
        }),
      notExists: (query: Types.SqlQuery) =>
        Expressions.makeExprNode({
          type: 'notExists',
          query: {
            text: query.text,
            raw: query.raw,
            values: query.values.map((v) =>
              RuntimeBridge.serializeSqlValue(v as Types.SqlValue),
            ),
          },
        }),
      and: (...conditions: Types.PredicateInput[]) =>
        Expressions.makeExprNode({
          type: 'and',
          conditions: conditions.map(
            Expressions.toExprNodeFromPredicate,
          ),
        }),
      or: (...conditions: Types.PredicateInput[]) =>
        Expressions.makeExprNode({
          type: 'or',
          conditions: conditions.map(
            Expressions.toExprNodeFromPredicate,
          ),
        }),
      count: (value = '*') =>
        exprFnCall('COUNT', [
          value as Types.SelectExpressionInput<TAvailable> | '*',
        ]),
      sum: (value) =>
        exprFnCall('SUM', [
          value as Types.SelectExpressionInput<TAvailable>,
        ]),
      avg: (value) =>
        exprFnCall('AVG', [
          value as Types.SelectExpressionInput<TAvailable>,
        ]),
      min: (value) =>
        exprFnCall('MIN', [
          value as Types.SelectExpressionInput<TAvailable>,
        ]),
      max: (value) =>
        exprFnCall('MAX', [
          value as Types.SelectExpressionInput<TAvailable>,
        ]),
      coalesce: (...values) =>
        exprFnCall(
          'COALESCE',
          values as Array<Types.SelectExpressionInput<TAvailable>>,
        ),
      case: (branches, elseValue) =>
        Expressions.makeExprNode({
          type: 'case',
          branches: branches.map((branch) => ({
            when: Expressions.toExprNodeFromPredicate(branch.when),
            result: Expressions.toExprOperand(branch.then),
          })),
          elseVal:
            elseValue !== undefined
              ? Expressions.toExprOperand(elseValue)
              : undefined,
        }),
      add: (left, right) =>
        Expressions.makeExprNode({
          type: 'arith',
          left: Expressions.toExprOperand(left),
          op: '+',
          right: Expressions.toExprOperand(right),
        }),
      sub: (left, right) =>
        Expressions.makeExprNode({
          type: 'arith',
          left: Expressions.toExprOperand(left),
          op: '-',
          right: Expressions.toExprOperand(right),
        }),
      mul: (left, right) =>
        Expressions.makeExprNode({
          type: 'arith',
          left: Expressions.toExprOperand(left),
          op: '*',
          right: Expressions.toExprOperand(right),
        }),
      div: (left, right) =>
        Expressions.makeExprNode({
          type: 'arith',
          left: Expressions.toExprOperand(left),
          op: '/',
          right: Expressions.toExprOperand(right),
        }),
    } satisfies Types.BuilderContext<TAvailable>;

    this._builderContextCache =
      context as unknown as Types.BuilderContext<string>;
    Builder.sharedBuilderContextCache.set(
      dialect,
      context as unknown as Types.BuilderContext<string>,
    );
    if (collector && startedAt !== undefined) {
      collector.recordStep(
        'builderContextCreateMs',
        collector.now() - startedAt,
      );
    }
    return context;
  }

  public ref<
    TColumn extends
      | Types.SelectedColumn<TSources, TDefaultColumns>
      | TSelectedColumns,
  >(column: Extract<TColumn, string>): Types.SqlIdentifier {
    return { __kind: 'identifier', parts: column.split('.') };
  }

  private resolveDatasetName(
    template: string,
    variables: Types.DatasetVariables,
  ) {
    return template.replace(/\$\{(.+?)\}/g, (_, key: string) => {
      const value = variables[key];

      if (!value) {
        throw new Error(`Missing dataset variable: ${key}`);
      }

      return value;
    });
  }

  /** @internal */
  public getDynamicDataset(
    // biome-ignore lint/suspicious/noExplicitAny: Dynamic datasets are inherently untyped
    lakehouse: any, // eslint-disable-line @typescript-eslint/no-explicit-any
    name: string,
    variables: Types.DatasetVariables,
  ) {
    const template = lakehouse.datasets.dynamic[name];

    if (!template) {
      throw new Error(`Unknown dynamic dataset: ${name}`);
    }

    for (const [key, definition] of Object.entries(
      template.variables,
    )) {
      const value = variables[key];

      if (!value) {
        throw new Error(`Missing variable: ${key}`);
      }

      if (
        (definition as unknown as { pattern?: RegExp }).pattern &&
        !(definition as unknown as { pattern?: RegExp }).pattern?.test(
          value,
        )
      ) {
        throw new Error(`Invalid value for variable: ${key}`);
      }
    }

    return {
      ...template,
      resolvedName: this.resolveDatasetName(
        template.datasetName,
        variables,
      ),
    };
  }

  private resolveCteCallback<
    TCallbackRegisteredSources extends Types.AnySourceColumnMap,
    TColumns extends string,
    TCallbackState extends Types.AnyBuilderState,
  >(
    callback: (
      db: Database<
        TSchema,
        TCallbackRegisteredSources,
        Types.EmptySourceColumnMap,
        never,
        never,
        Types.InitialBuilderState
      >,
    ) => Database<
      TSchema,
      Types.AnySourceColumnMap,
      Types.AnySourceColumnMap,
      string,
      TColumns,
      TCallbackState
    >,
  ): Database<
    TSchema,
    Types.AnySourceColumnMap,
    Types.AnySourceColumnMap,
    string,
    TColumns,
    TCallbackState
  > {
    return Diagnostics.QueryDiagnostics.measureBuildDiagnosticsStep(
      Diagnostics.DiagnosticsCollector.Builder(),
      'callbackResolveMs',
      () =>
        callback(
          Database._ephemeral<TSchema, TConnection>({
            dialect: this.dialect,
            connection: this.connection,
          } as Types.DatabaseConfig<TSchema, TConnection>) as Database<
            TSchema,
            TCallbackRegisteredSources,
            Types.EmptySourceColumnMap,
            never,
            never,
            Types.InitialBuilderState,
            TConnection
          >,
        ),
    );
  }

  public with<
    TName extends string,
    TColumns extends string,
    TCallbackState extends Types.AnyBuilderState,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.CteCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState
      >,
    name: TName,
    callback: (
      db: Database<
        TSchema,
        Types.EmptySourceColumnMap,
        Types.EmptySourceColumnMap,
        never,
        never,
        Types.InitialBuilderState
      >,
    ) => Database<
      TSchema,
      Types.AnySourceColumnMap,
      Types.AnySourceColumnMap,
      string,
      TColumns,
      TCallbackState
    >,
  ): Database<
    TSchema,
    Types.MergeSources<
      TRegisteredSources,
      Types.NamedSourceMap<TName, TColumns>
    >,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithCte
  > {
    const query = this.resolveCteCallback(callback);
    return this.cloneWithPendingOp<
      Types.MergeSources<
        TRegisteredSources,
        Types.NamedSourceMap<TName, TColumns>
      >,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithCte
    >({
      op: 'withBuilder',
      name,
      rhsBuilder: query as Types.AnyDatabaseInstance,
    }) as Database<
      TSchema,
      Types.MergeSources<
        TRegisteredSources,
        Types.NamedSourceMap<TName, TColumns>
      >,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithCte
    >;
  }

  public withRecursive<
    TName extends string,
    TColumns extends string,
    TCallbackState extends Types.AnyBuilderState,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.CteCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState
      >,
    name: TName,
    callback: (
      db: Database<
        TSchema,
        Types.MergeSources<
          TRegisteredSources,
          Types.NamedSourceMap<TName, TColumns>
        >,
        Types.EmptySourceColumnMap,
        never,
        never,
        Types.InitialBuilderState
      >,
    ) => Database<
      TSchema,
      Types.AnySourceColumnMap,
      Types.AnySourceColumnMap,
      string,
      TColumns,
      TCallbackState
    >,
  ): Database<
    TSchema,
    Types.MergeSources<
      TRegisteredSources,
      Types.NamedSourceMap<TName, TColumns>
    >,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithCte
  > {
    const query = this.resolveCteCallback(callback);
    return this.cloneWithPendingOp<
      Types.MergeSources<
        TRegisteredSources,
        Types.NamedSourceMap<TName, TColumns>
      >,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithCte
    >({
      op: 'withRecursiveBuilder',
      name,
      rhsBuilder: query as Types.AnyDatabaseInstance,
    }) as Database<
      TSchema,
      Types.MergeSources<
        TRegisteredSources,
        Types.NamedSourceMap<TName, TColumns>
      >,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithCte
    >;
  }

  public insertInto<
    TTable extends Types.AvailableSourceName<
      TSchema,
      Types.EmptySourceColumnMap
    >,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.InsertIntoCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    table: TTable,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.EmptySourceColumnMap,
    Types.AvailableSourceColumns<
      TSchema,
      Types.EmptySourceColumnMap,
      TTable
    >,
    never,
    Types.WithInsertInto,
    TConnection
  > {
    return this.applyDirectMutation<
      TRegisteredSources,
      Types.EmptySourceColumnMap,
      Types.AvailableSourceColumns<
        TSchema,
        Types.EmptySourceColumnMap,
        TTable
      >,
      never,
      Types.WithInsertInto
    >((handle) =>
      RuntimeBridge.runtimeBuilderInsertInto(handle, table),
    ) as unknown as Database<
      TSchema,
      TRegisteredSources,
      Types.EmptySourceColumnMap,
      Types.AvailableSourceColumns<
        TSchema,
        Types.EmptySourceColumnMap,
        TTable
      >,
      never,
      Types.WithInsertInto,
      TConnection
    >;
  }

  public columns<TColumn extends TDefaultColumns>(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.InsertColumnCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    columns: [TColumn, ...TColumn[]],
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithInsertColumns,
    TConnection
  > {
    return this.applyDirectMutation<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithInsertColumns
    >((handle) =>
      RuntimeBridge.runtimeBuilderColumns(
        handle,
        JSON.stringify(columns),
      ),
    );
  }

  public update<
    TTable extends Types.AvailableSourceName<
      TSchema,
      Types.EmptySourceColumnMap
    >,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.UpdateCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    table: TTable,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.AvailableSourceMap<
      TSchema,
      Types.EmptySourceColumnMap,
      TTable,
      TTable
    >,
    Types.AvailableSourceColumns<
      TSchema,
      Types.EmptySourceColumnMap,
      TTable
    >,
    never,
    Types.WithUpdate,
    TConnection
  > {
    return this.applyDirectMutation<
      TRegisteredSources,
      Types.AvailableSourceMap<
        TSchema,
        Types.EmptySourceColumnMap,
        TTable,
        TTable
      >,
      Types.AvailableSourceColumns<
        TSchema,
        Types.EmptySourceColumnMap,
        TTable
      >,
      never,
      Types.WithUpdate
    >((handle) =>
      RuntimeBridge.runtimeBuilderUpdate(handle, table),
    ) as unknown as Database<
      TSchema,
      TRegisteredSources,
      Types.AvailableSourceMap<
        TSchema,
        Types.EmptySourceColumnMap,
        TTable,
        TTable
      >,
      Types.AvailableSourceColumns<
        TSchema,
        Types.EmptySourceColumnMap,
        TTable
      >,
      never,
      Types.WithUpdate,
      TConnection
    >;
  }

  public deleteFrom<
    TTable extends Types.AvailableSourceName<
      TSchema,
      Types.EmptySourceColumnMap
    >,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.DeleteCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    table: TTable,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.AvailableSourceMap<
      TSchema,
      Types.EmptySourceColumnMap,
      TTable,
      TTable
    >,
    Types.AvailableSourceColumns<
      TSchema,
      Types.EmptySourceColumnMap,
      TTable
    >,
    never,
    Types.WithDelete,
    TConnection
  > {
    return this.applyDirectMutation<
      TRegisteredSources,
      Types.AvailableSourceMap<
        TSchema,
        Types.EmptySourceColumnMap,
        TTable,
        TTable
      >,
      Types.AvailableSourceColumns<
        TSchema,
        Types.EmptySourceColumnMap,
        TTable
      >,
      never,
      Types.WithDelete
    >((handle) =>
      RuntimeBridge.runtimeBuilderDeleteFrom(handle, table),
    ) as unknown as Database<
      TSchema,
      TRegisteredSources,
      Types.AvailableSourceMap<
        TSchema,
        Types.EmptySourceColumnMap,
        TTable,
        TTable
      >,
      Types.AvailableSourceColumns<
        TSchema,
        Types.EmptySourceColumnMap,
        TTable
      >,
      never,
      Types.WithDelete,
      TConnection
    >;
  }

  public distinct(
    this: Gate<
      ThisDb<
        TSchema,
        TConnection,
        TState,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns
      >,
      Types.DistinctCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >
    >,
  ): ThisDb<
    TSchema,
    TConnection,
    Types.WithDistinct<TState>,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns
  > {
    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithDistinct<TState>
    >({
      op: 'distinct',
    });
  }

  public distinctOn(
    this: Gate<
      ThisDb<
        TSchema,
        TConnection,
        TState,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns
      >,
      Types.DistinctCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >
    >,
    columns: Types.DistinctOnInput<TSources, TDefaultColumns>,
  ): ThisDb<
    TSchema,
    TConnection,
    Types.WithDistinct<TState>,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns
  >;

  public distinctOn(
    this: Gate<
      ThisDb<
        TSchema,
        TConnection,
        TState,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns
      >,
      Types.DistinctCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >
    >,
    columns: (
      db: Types.BuilderContext<
        Types.SelectedColumn<TSources, TDefaultColumns>
      >,
    ) => Types.DistinctOnInput<TSources, TDefaultColumns>,
  ): ThisDb<
    TSchema,
    TConnection,
    Types.WithDistinct<TState>,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns
  >;

  public distinctOn(
    this: Gate<
      ThisDb<
        TSchema,
        TConnection,
        TState,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns
      >,
      Types.DistinctCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >
    >,
    columns:
      | Types.DistinctOnInput<TSources, TDefaultColumns>
      | ((
          db: Types.BuilderContext<
            Types.SelectedColumn<TSources, TDefaultColumns>
          >,
        ) => Types.DistinctOnInput<TSources, TDefaultColumns>),
  ): ThisDb<
    TSchema,
    TConnection,
    Types.WithDistinct<TState>,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns
  > {
    return this.applyDistinctOnInput(
      this.resolveDistinctOnInput(columns),
    );
  }

  public select<TColumns extends string>(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.InsertColumnCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    query: Types.QueryOperand<TColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithInsertSelect,
    TConnection
  >;

  public select<
    TColumns extends Types.SelectInput<TSources, TDefaultColumns>,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.SelectableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState
      >,
    columns: TColumns,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    Types.SelectedOutputColumns<TColumns>,
    Types.WithSelect<TState>
  >;

  public select<
    TColumns extends Types.SelectInput<TSources, TDefaultColumns>,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.SelectableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState
      >,
    columns: (
      db: Types.BuilderContext<
        Types.SelectedColumn<TSources, TDefaultColumns>
      >,
    ) => TColumns,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    Types.SelectedOutputColumns<TColumns>,
    Types.WithSelect<TState>
  >;

  public select<
    TInsertColumns extends string,
    TColumns extends Types.SelectInput<TSources, TDefaultColumns>,
  >(
    columns:
      | TColumns
      | ((
          db: Types.BuilderContext<
            Types.SelectedColumn<TSources, TDefaultColumns>
          >,
        ) => TColumns)
      | Types.QueryOperand<TInsertColumns>,
  ):
    | Database<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        Types.WithInsertSelect,
        TConnection
      >
    | Database<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        Types.SelectedOutputColumns<TColumns>,
        Types.WithSelect<TState>
      > {
    // INSERT...SELECT overload: columns is a Types.QueryOperand (has .query property)
    if (
      typeof columns === 'object' &&
      columns !== null &&
      'query' in columns &&
      !Array.isArray(columns)
    ) {
      const queryOperand = columns as Types.AnyQueryOperand;
      // Resolve insert columns without forcing an eager commit: check pending ops first,
      // then fall back to the already-committed base handle.
      const pendingInsertColsOp = this._pendingOps.findLast(
        (op) => op.op === 'insertColumns',
      ) as { op: 'insertColumns'; cols: string[] } | undefined;
      const insertCols =
        pendingInsertColsOp?.cols ??
        RuntimeBridge.runtimeBuilderInsertColumns(this.committedHandle);
      if (insertCols.length === 0) {
        throw new Error(
          'insertInto(...).select(...) requires columns(...) first.',
        );
      }
      const selectedCols =
        this.resolveQueryOperandColumns(queryOperand);
      if (selectedCols && selectedCols.length !== insertCols.length) {
        throw new Error(
          'insertInto(...).select(...) column count must match columns(...).',
        );
      }
      return this.applyDirectMutation<
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        Types.WithInsertSelect
      >((handle) =>
        queryOperand instanceof Database
          ? RuntimeBridge.runtimeBuilderInsertSelectHandle(
              handle,
              queryOperand.committedHandle,
            )
          : RuntimeBridge.runtimeBuilderInsertSelect(
              handle,
              JSON.stringify(
                RuntimeBridge.serializeQuery(queryOperand.query),
              ),
            ),
      );
    }

    const resolvedColumns = this.resolveSelectInput(
      columns as
        | TColumns
        | ((
            db: Types.BuilderContext<
              Types.SelectedColumn<TSources, TDefaultColumns>
            >,
          ) => TColumns),
    );
    return this.applyProjectionInput<
      TColumns,
      Types.SelectedOutputColumns<TColumns>,
      Types.WithSelect<TState>
    >(resolvedColumns, {
      aliased: 'selectAliased',
      columns: 'selectColumns',
    });
  }

  public returning<
    TColumns extends Types.SelectInput<TSources, TDefaultColumns>,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.ReturningCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    columns: TColumns,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    Types.SelectedOutputColumns<TColumns>,
    Types.WithReturning,
    TConnection
  >;

  public returning<
    TColumns extends Types.SelectInput<TSources, TDefaultColumns>,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.ReturningCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    columns: (
      db: Types.BuilderContext<
        Types.SelectedColumn<TSources, TDefaultColumns>
      >,
    ) => TColumns,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    Types.SelectedOutputColumns<TColumns>,
    Types.WithReturning,
    TConnection
  >;

  public returning<
    TColumns extends Types.SelectInput<TSources, TDefaultColumns>,
  >(
    columns:
      | TColumns
      | ((
          db: Types.BuilderContext<
            Types.SelectedColumn<TSources, TDefaultColumns>
          >,
        ) => TColumns),
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    Types.SelectedOutputColumns<TColumns>,
    Types.WithReturning,
    TConnection
  > {
    const resolvedColumns = this.resolveSelectInput(
      columns as
        | TColumns
        | ((
            db: Types.BuilderContext<
              Types.SelectedColumn<TSources, TDefaultColumns>
            >,
          ) => TColumns),
    );
    return this.applyProjectionInput<
      TColumns,
      Types.SelectedOutputColumns<TColumns>,
      Types.WithReturning
    >(resolvedColumns, {
      aliased: 'returningAliased',
      columns: 'returningColumns',
    });
  }

  public values(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.CompleteQueryDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
  ): Types.Primitive[];

  public values<TColumn extends TDefaultColumns>(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.InsertValuesCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    rows: Types.InsertRow<TColumn> | Array<Types.InsertRow<TColumn>>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithInsertValues,
    TConnection
  >;

  public values<TColumn extends TDefaultColumns>(
    rows?: Types.InsertRow<TColumn> | Array<Types.InsertRow<TColumn>>,
  ):
    | Types.Primitive[]
    | Database<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        Types.WithInsertValues,
        TConnection
      > {
    if (rows === undefined) {
      return RuntimeBridge.runtimeBuilderValues(this.committedHandle);
    }

    const explicitColumns = RuntimeBridge.runtimeBuilderInsertColumns(
      this.committedHandle,
    );
    const normalized = this.normalizeInsertRows(rows, explicitColumns);

    return this.applyDirectMutation<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithInsertValues
    >((handle) =>
      RuntimeBridge.runtimeBuilderValuesInsert(
        handle,
        JSON.stringify({
          columnNames: normalized.columnNames,
          rows: normalized.rows,
        }),
      ),
    );
  }

  public onConflict<TColumn extends TDefaultColumns>(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.InsertConflictTargetCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    columns: [TColumn, ...TColumn[]],
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithInsertConflictTarget,
    TConnection
  > {
    if (
      RuntimeBridge.runtimeBuilderConflictTargetKind(
        this.committedHandle,
      ) === 'constraint'
    ) {
      throw new Error(
        'Insert conflict target cannot mix columns and constraint targets.',
      );
    }
    return this.applyDirectMutation<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithInsertConflictTarget
    >((handle) =>
      RuntimeBridge.runtimeBuilderOnConflictColumns(
        handle,
        JSON.stringify(columns),
      ),
    );
  }

  public onConflictOnConstraint(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.InsertConflictTargetCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    name: string,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithInsertConflictTarget,
    TConnection
  > {
    if (!name) {
      throw new Error(
        'onConflictOnConstraint(...) requires a non-empty constraint name.',
      );
    }
    if (
      RuntimeBridge.runtimeBuilderConflictTargetKind(
        this.committedHandle,
      ) === 'columns'
    ) {
      throw new Error(
        'Insert conflict target cannot mix columns and constraint targets.',
      );
    }
    return this.applyDirectMutation<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithInsertConflictTarget
    >((handle) =>
      RuntimeBridge.runtimeBuilderOnConflictConstraint(handle, name),
    );
  }

  public doNothing(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.InsertConflictActionCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithInsertConflictAction,
    TConnection
  > {
    return this.applyDirectMutation<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithInsertConflictAction
    >((handle) => RuntimeBridge.runtimeBuilderDoNothing(handle));
  }

  public doUpdateSet<TColumn extends TDefaultColumns>(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.InsertConflictActionCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    patch: Types.UpdateSet<TColumn>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithInsertConflictUpdate,
    TConnection
  > {
    const entries = Object.entries(patch);

    if (entries.length === 0) {
      throw new Error('doUpdateSet(...) requires at least one column.');
    }

    const assignments = entries.map(([column, value]) => [
      column,
      RuntimeBridge.serializeSqlValue(
        InternalQueryEngine.toWriteValue(
          value as Types.WriteValue,
        ) as Types.SqlValue,
      ),
    ]) as [string, unknown][];

    return this.applyDirectMutation<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithInsertConflictUpdate
    >((handle) =>
      RuntimeBridge.runtimeBuilderDoUpdateSet(
        handle,
        JSON.stringify(assignments),
      ),
    );
  }

  public conflictWhere(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.InsertConflictWhereCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    condition: Types.PredicateResolverInput<TSources, TDefaultColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithInsertConflictAction,
    TConnection
  > {
    const resolved =
      this.resolvePredicateInput<
        Types.SelectedColumn<TSources, TDefaultColumns>
      >(condition);

    return this.applyDirectMutation<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithInsertConflictAction
    >((handle) =>
      RuntimeBridge.runtimeBuilderConflictWhere(
        handle,
        JSON.stringify(
          Builder.buildPredicateForOp(
            typeof resolved === 'string'
              ? { text: resolved, raw: resolved, values: [] }
              : resolved,
          ),
        ),
      ),
    );
  }

  public set<TColumn extends TDefaultColumns>(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.SetCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    patch: Types.UpdateSet<TColumn>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithSet,
    TConnection
  > {
    const entries = Object.entries(patch);

    if (entries.length === 0) {
      throw new Error('set(...) requires at least one column.');
    }

    const assignments = entries.map(([column, value]) => [
      column,
      RuntimeBridge.serializeSqlValue(
        InternalQueryEngine.toWriteValue(
          value as Types.WriteValue,
        ) as Types.SqlValue,
      ),
    ]) as [string, unknown][];

    return this.applyDirectMutation<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithSet
    >((handle) =>
      RuntimeBridge.runtimeBuilderSet(
        handle,
        JSON.stringify(assignments),
      ),
    );
  }

  public from<
    TSource extends Types.AvailableSourceName<
      TSchema,
      TRegisteredSources
    >,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.FromCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: TSource,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.AvailableSourceMap<
      TSchema,
      TRegisteredSources,
      TSource,
      TSource
    >,
    Types.AvailableSourceColumns<TSchema, TRegisteredSources, TSource>,
    never,
    Types.WithFrom<TState>,
    TConnection
  >;

  public from<
    TSource extends Types.AvailableSourceName<
      TSchema,
      TRegisteredSources
    >,
    TAlias extends string,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.FromCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: TSource,
    alias: TAlias,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.AvailableSourceMap<
      TSchema,
      TRegisteredSources,
      TSource,
      TAlias
    >,
    never,
    never,
    Types.WithFrom<TState>,
    TConnection
  >;

  public from<TAlias extends string, TColumns extends string>(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.FromCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    query: Types.AliasedQuery<TAlias, TColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.NamedSourceMap<TAlias, TColumns>,
    never,
    never,
    Types.WithFrom<TState>,
    TConnection
  >;

  public from(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.FromCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: string | Types.AnyAliasedQuery,
    alias?: string,
  ): Database<
    TSchema,
    TRegisteredSources,
    any,
    any,
    never,
    Types.WithFrom<TState>,
    TConnection
  > {
    if (InternalQueryEngine.isAliasedQuery(source)) {
      const rhsBuilder =
        InternalQueryEngine.getAliasedQueryBuilder(source);
      if (rhsBuilder) {
        return this.cloneWithPendingOp<
          TRegisteredSources,
          Types.AnySourceColumnMap,
          string,
          never,
          Types.WithFrom<TState>
        >({
          op: 'fromSubqueryBuilder',
          alias: source.alias,
          rhsBuilder,
        }) as unknown as Database<
          TSchema,
          TRegisteredSources,
          Types.AnySourceColumnMap,
          string,
          never,
          Types.WithFrom<TState>,
          TConnection
        >;
      }
      const rhsHandle =
        InternalQueryEngine.getAliasedQueryHandle(source);
      return (rhsHandle
        ? this.cloneWithPendingOp<
            TRegisteredSources,
            Types.AnySourceColumnMap,
            string,
            never,
            Types.WithFrom<TState>
          >({
            op: 'fromSubqueryHandle',
            alias: source.alias,
            rhsHandle,
          })
        : this.cloneWithPendingOp<
            TRegisteredSources,
            Types.AnySourceColumnMap,
            string,
            never,
            Types.WithFrom<TState>
          >({
            op: 'fromSubquery',
            alias: source.alias,
            query: RuntimeBridge.serializeQuery(source.query),
          })) as unknown as Database<
        TSchema,
        TRegisteredSources,
        Types.AnySourceColumnMap,
        string,
        never,
        Types.WithFrom<TState>,
        TConnection
      >;
    }

    return (alias
      ? this.cloneWithPendingOp<
          TRegisteredSources,
          Types.AnySourceColumnMap,
          string,
          never,
          Types.WithFrom<TState>
        >({
          op: 'fromTableAlias',
          table: source,
          alias,
        })
      : this.cloneWithPendingOp<
          TRegisteredSources,
          Types.AnySourceColumnMap,
          string,
          never,
          Types.WithFrom<TState>
        >({
          op: 'fromTable',
          table: source,
        })) as unknown as Database<
      TSchema,
      TRegisteredSources,
      Types.AnySourceColumnMap,
      string,
      never,
      Types.WithFrom<TState>,
      TConnection
    >;
  }

  public join<
    TSource extends Types.AvailableSourceName<
      TSchema,
      TRegisteredSources
    >,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: TSource,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.MergeSources<
      TSources,
      Types.AvailableSourceMap<
        TSchema,
        TRegisteredSources,
        TSource,
        TSource
      >
    >,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoin<TState>,
    TConnection
  >;

  public join<
    TSource extends Types.AvailableSourceName<
      TSchema,
      TRegisteredSources
    >,
    TAlias extends string,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: TSource,
    alias: TAlias,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.MergeSources<
      TSources,
      Types.AvailableSourceMap<
        TSchema,
        TRegisteredSources,
        TSource,
        TAlias
      >
    >,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoin<TState>,
    TConnection
  >;

  public join<TAlias extends string, TColumns extends string>(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    query: Types.AliasedQuery<TAlias, TColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.MergeSources<
      TSources,
      Types.NamedSourceMap<TAlias, TColumns>
    >,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoin<TState>,
    TConnection
  >;

  public join(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: string | Types.AnyAliasedQuery,
    alias?: string,
  ): Database<
    TSchema,
    TRegisteredSources,
    any,
    any,
    any,
    Types.WithPendingJoin<TState>,
    TConnection
  > {
    return this.joinHelper(
      'INNER JOIN',
      source,
      alias,
    ) as unknown as Database<
      TSchema,
      TRegisteredSources,
      Types.AnySourceColumnMap,
      string,
      string,
      Types.WithPendingJoin<TState>,
      TConnection
    >;
  }

  public innerJoin = this.join;

  private joinHelper(
    joinType: string,
    source: string | Types.AnyAliasedQuery,
    alias?: string,
  ): Types.AnyDatabaseInstance {
    if (InternalQueryEngine.isAliasedQuery(source)) {
      const rhsBuilder =
        InternalQueryEngine.getAliasedQueryBuilder(source);
      if (rhsBuilder) {
        return this.cloneWithPendingOp({
          op: 'joinSubqueryBuilder',
          joinType,
          alias: source.alias,
          rhsBuilder,
        });
      }
      const rhsHandle =
        InternalQueryEngine.getAliasedQueryHandle(source);
      return rhsHandle
        ? this.cloneWithPendingOp({
            op: 'joinSubqueryHandle',
            joinType,
            alias: source.alias,
            rhsHandle,
          })
        : this.cloneWithPendingOp({
            op: 'joinSubquery',
            joinType,
            alias: source.alias,
            query: RuntimeBridge.serializeQuery(source.query),
          });
    }

    return alias
      ? this.cloneWithPendingOp({
          op: 'joinTableAlias',
          joinType,
          table: source,
          alias,
        })
      : this.cloneWithPendingOp({
          op: 'joinTable',
          joinType,
          table: source,
        });
  }

  public leftJoin<
    TSource extends Types.AvailableSourceName<
      TSchema,
      TRegisteredSources
    >,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: TSource,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.MergeSources<
      TSources,
      Types.AvailableSourceMap<
        TSchema,
        TRegisteredSources,
        TSource,
        TSource
      >
    >,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoin<TState>,
    TConnection
  >;

  public leftJoin<
    TSource extends Types.AvailableSourceName<
      TSchema,
      TRegisteredSources
    >,
    TAlias extends string,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: TSource,
    alias: TAlias,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.MergeSources<
      TSources,
      Types.AvailableSourceMap<
        TSchema,
        TRegisteredSources,
        TSource,
        TAlias
      >
    >,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoin<TState>,
    TConnection
  >;

  public leftJoin<TAlias extends string, TColumns extends string>(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    query: Types.AliasedQuery<TAlias, TColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.MergeSources<
      TSources,
      Types.NamedSourceMap<TAlias, TColumns>
    >,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoin<TState>,
    TConnection
  >;

  public leftJoin(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: string | Types.AnyAliasedQuery,
    alias?: string,
  ): Database<
    TSchema,
    TRegisteredSources,
    any,
    any,
    any,
    Types.WithPendingJoin<TState>,
    TConnection
  > {
    return this.joinHelper(
      'LEFT JOIN',
      source,
      alias,
    ) as unknown as Database<
      TSchema,
      TRegisteredSources,
      Types.AnySourceColumnMap,
      string,
      string,
      Types.WithPendingJoin<TState>,
      TConnection
    >;
  }

  public rightJoin<
    TSource extends Types.AvailableSourceName<
      TSchema,
      TRegisteredSources
    >,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: TSource,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.MergeSources<
      TSources,
      Types.AvailableSourceMap<
        TSchema,
        TRegisteredSources,
        TSource,
        TSource
      >
    >,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoin<TState>,
    TConnection
  >;

  public rightJoin<
    TSource extends Types.AvailableSourceName<
      TSchema,
      TRegisteredSources
    >,
    TAlias extends string,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: TSource,
    alias: TAlias,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.MergeSources<
      TSources,
      Types.AvailableSourceMap<
        TSchema,
        TRegisteredSources,
        TSource,
        TAlias
      >
    >,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoin<TState>,
    TConnection
  >;

  public rightJoin<TAlias extends string, TColumns extends string>(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    query: Types.AliasedQuery<TAlias, TColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.MergeSources<
      TSources,
      Types.NamedSourceMap<TAlias, TColumns>
    >,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoin<TState>,
    TConnection
  >;

  public rightJoin(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: string | Types.AnyAliasedQuery,
    alias?: string,
  ): Database<
    TSchema,
    TRegisteredSources,
    any,
    any,
    any,
    Types.WithPendingJoin<TState>,
    TConnection
  > {
    return this.joinHelper(
      'RIGHT JOIN',
      source,
      alias,
    ) as unknown as Database<
      TSchema,
      TRegisteredSources,
      Types.AnySourceColumnMap,
      string,
      string,
      Types.WithPendingJoin<TState>,
      TConnection
    >;
  }

  public fullJoin<
    TSource extends Types.AvailableSourceName<
      TSchema,
      TRegisteredSources
    >,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: TSource,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.MergeSources<
      TSources,
      Types.AvailableSourceMap<
        TSchema,
        TRegisteredSources,
        TSource,
        TSource
      >
    >,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoin<TState>,
    TConnection
  >;

  public fullJoin<
    TSource extends Types.AvailableSourceName<
      TSchema,
      TRegisteredSources
    >,
    TAlias extends string,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: TSource,
    alias: TAlias,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.MergeSources<
      TSources,
      Types.AvailableSourceMap<
        TSchema,
        TRegisteredSources,
        TSource,
        TAlias
      >
    >,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoin<TState>,
    TConnection
  >;

  public fullJoin<TAlias extends string, TColumns extends string>(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    query: Types.AliasedQuery<TAlias, TColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.MergeSources<
      TSources,
      Types.NamedSourceMap<TAlias, TColumns>
    >,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoin<TState>,
    TConnection
  >;

  public fullJoin(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: string | Types.AnyAliasedQuery,
    alias?: string,
  ): Database<
    TSchema,
    TRegisteredSources,
    any,
    any,
    any,
    Types.WithPendingJoin<TState>,
    TConnection
  > {
    return this.joinHelper(
      'FULL JOIN',
      source,
      alias,
    ) as unknown as Database<
      TSchema,
      TRegisteredSources,
      Types.AnySourceColumnMap,
      string,
      string,
      Types.WithPendingJoin<TState>,
      TConnection
    >;
  }

  public crossJoin<
    TSource extends Types.AvailableSourceName<
      TSchema,
      TRegisteredSources
    >,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: TSource,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.MergeSources<
      TSources,
      Types.AvailableSourceMap<
        TSchema,
        TRegisteredSources,
        TSource,
        TSource
      >
    >,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithJoin<TState>,
    TConnection
  >;

  public crossJoin<
    TSource extends Types.AvailableSourceName<
      TSchema,
      TRegisteredSources
    >,
    TAlias extends string,
  >(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: TSource,
    alias: TAlias,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.MergeSources<
      TSources,
      Types.AvailableSourceMap<
        TSchema,
        TRegisteredSources,
        TSource,
        TAlias
      >
    >,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithJoin<TState>,
    TConnection
  >;

  public crossJoin<TAlias extends string, TColumns extends string>(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    query: Types.AliasedQuery<TAlias, TColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    Types.MergeSources<
      TSources,
      Types.NamedSourceMap<TAlias, TColumns>
    >,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithJoin<TState>,
    TConnection
  >;

  public crossJoin(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    source: string | Types.AnyAliasedQuery,
    alias?: string,
  ): Database<
    TSchema,
    TRegisteredSources,
    any,
    any,
    any,
    Types.WithJoin<TState>,
    TConnection
  > {
    return this.joinHelper(
      'CROSS JOIN',
      source,
      alias,
    ) as unknown as Database<
      TSchema,
      TRegisteredSources,
      Types.AnySourceColumnMap,
      string,
      string,
      Types.WithJoin<TState>,
      TConnection
    >;
  }

  public on(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinPredicateDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    condition: Types.PredicateInput,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoinReady<TState>
  >;

  public on(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinPredicateDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    condition: (
      db: Types.BuilderContext<
        Types.SelectedColumn<TSources, TDefaultColumns>
      >,
    ) => Types.PredicateInput,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoinReady<TState>
  >;

  public on(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinPredicateDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    left: Types.SelectedColumn<TSources, never>,
    operator: Types.ComparisonOperator,
    right:
      | Types.SelectedColumn<TSources, never>
      | Exclude<Types.Primitive, string>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoinReady<TState>
  >;

  public on(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinPredicateDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    first:
      | Types.PredicateResolverInput<TSources, TDefaultColumns>
      | Types.JoinOnLeft<TSources>,
    second?: Types.ComparisonOperator,
    third?: Types.JoinOnRight<TSources>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoinReady<TState>,
    TConnection
  > {
    const condition =
      second && third !== undefined && typeof first === 'string'
        ? Core.sql`${Core.identifier(first)} ${Core.raw(second)} ${typeof third === 'string' && third.includes('.') ? Core.identifier(third) : third}`
        : this.resolvePredicateInput<
            Types.SelectedColumn<TSources, TDefaultColumns>
          >(first);
    const q: Types.SqlQuery =
      typeof condition === 'string'
        ? InternalQueryEngine.makeStaticSqlQuery(condition)
        : condition;

    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithPendingJoinReady<TState>
    >({
      op: 'on',
      pred: Builder.buildPredicateForOp(q),
    });
  }

  public andOn(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinPredicateDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    condition: Types.PredicateInput,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoinReady<TState>
  > {
    const andOnResolved =
      this.resolvePredicateInput<
        Types.SelectedColumn<TSources, TDefaultColumns>
      >(condition);
    const andOnQ: Types.SqlQuery =
      typeof andOnResolved === 'string'
        ? InternalQueryEngine.makeStaticSqlQuery(andOnResolved)
        : andOnResolved;
    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithPendingJoinReady<TState>
    >({
      op: 'andOn',
      pred: Builder.buildPredicateForOp(andOnQ),
    });
  }

  public orOn(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinPredicateDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    condition: Types.PredicateInput,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoinReady<TState>
  > {
    const orOnResolved =
      this.resolvePredicateInput<
        Types.SelectedColumn<TSources, TDefaultColumns>
      >(condition);
    const orOnQ: Types.SqlQuery =
      typeof orOnResolved === 'string'
        ? InternalQueryEngine.makeStaticSqlQuery(orOnResolved)
        : orOnResolved;
    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithPendingJoinReady<TState>
    >({
      op: 'orOn',
      pred: Builder.buildPredicateForOp(orOnQ),
    });
  }

  public using(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinPredicateDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    columns: [
      Types.SourceColumnName<TSources>,
      ...Array<Types.SourceColumnName<TSources>>,
    ],
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoinReady<TState>
  > {
    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithPendingJoinReady<TState>
    >({
      op: 'usingColumns',
      cols: [...columns],
    });
  }

  public onColumns(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.JoinPredicateDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    pairs: [
      [
        Types.SelectedColumn<TSources, never>,
        Types.SelectedColumn<TSources, never>,
      ],
      ...Array<
        [
          Types.SelectedColumn<TSources, never>,
          Types.SelectedColumn<TSources, never>,
        ]
      >,
    ],
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithPendingJoinReady<TState>
  > {
    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithPendingJoinReady<TState>
    >({
      op: 'onColumns',
      pairs,
    });
  }

  public where(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.WhereCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    condition: Types.PredicateInput,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithWhere<TState>
  >;

  public where(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.WhereCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    condition: (
      db: Types.BuilderContext<
        Types.SelectedColumn<TSources, TDefaultColumns>
      >,
    ) => Types.PredicateInput,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithWhere<TState>
  >;

  public where(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.WhereCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    condition:
      | Types.PredicateInput
      | ((
          db: Types.BuilderContext<
            Types.SelectedColumn<TSources, TDefaultColumns>
          >,
        ) => Types.PredicateInput),
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithWhere<TState>
  > {
    const whereResolved =
      this.resolvePredicateInput<
        Types.SelectedColumn<TSources, TDefaultColumns>
      >(condition);
    const whereQ: Types.SqlQuery =
      typeof whereResolved === 'string'
        ? InternalQueryEngine.makeStaticSqlQuery(whereResolved)
        : whereResolved;

    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithWhere<TState>
    >({
      op: 'where',
      pred: Builder.buildPredicateForOp(whereQ),
    });
  }

  public andWhere(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.WhereCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    condition: Types.PredicateResolverInput<TSources, TDefaultColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithWhere<TState>
  > {
    const andWhereResolved =
      this.resolvePredicateInput<
        Types.SelectedColumn<TSources, TDefaultColumns>
      >(condition);
    const andWhereQ: Types.SqlQuery =
      typeof andWhereResolved === 'string'
        ? InternalQueryEngine.makeStaticSqlQuery(andWhereResolved)
        : andWhereResolved;

    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithWhere<TState>
    >({
      op: 'andWhere',
      pred: Builder.buildPredicateForOp(andWhereQ),
    });
  }

  public orWhere(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.WhereCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    condition: Types.PredicateResolverInput<TSources, TDefaultColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithWhere<TState>
  > {
    const orWhereResolved =
      this.resolvePredicateInput<
        Types.SelectedColumn<TSources, TDefaultColumns>
      >(condition);
    const orWhereQ: Types.SqlQuery =
      typeof orWhereResolved === 'string'
        ? InternalQueryEngine.makeStaticSqlQuery(orWhereResolved)
        : orWhereResolved;

    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithWhere<TState>
    >({
      op: 'orWhere',
      pred: Builder.buildPredicateForOp(orWhereQ),
    });
  }

  public groupBy(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.GroupByCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState
      >,
    columns: Array<Types.SelectedColumn<TSources, TDefaultColumns>>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithGroupBy<TState>
  > {
    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithGroupBy<TState>
    >({
      op: 'groupBy',
      cols: columns,
    });
  }

  public having(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.HavingCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    condition: Types.PredicateInput,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithHaving<TState>
  >;

  public having(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.HavingCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    condition: (
      db: Types.BuilderContext<
        Types.SelectedColumn<TSources, TDefaultColumns>
      >,
    ) => Types.PredicateInput,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithHaving<TState>
  >;

  public having(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.HavingCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    condition:
      | Types.PredicateInput
      | ((
          db: Types.BuilderContext<
            Types.SelectedColumn<TSources, TDefaultColumns>
          >,
        ) => Types.PredicateInput),
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithHaving<TState>
  > {
    const havingResolved =
      this.resolvePredicateInput<
        Types.SelectedColumn<TSources, TDefaultColumns>
      >(condition);
    const havingQ: Types.SqlQuery =
      typeof havingResolved === 'string'
        ? InternalQueryEngine.makeStaticSqlQuery(havingResolved)
        : havingResolved;

    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithHaving<TState>
    >({
      op: 'having',
      pred: Builder.buildPredicateForOp(havingQ),
    });
  }

  public andHaving(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.HavingCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    condition: Types.PredicateResolverInput<TSources, TDefaultColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithHaving<TState>
  > {
    const andHavingResolved =
      this.resolvePredicateInput<
        Types.SelectedColumn<TSources, TDefaultColumns>
      >(condition);
    const andHavingQ: Types.SqlQuery =
      typeof andHavingResolved === 'string'
        ? InternalQueryEngine.makeStaticSqlQuery(andHavingResolved)
        : andHavingResolved;

    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithHaving<TState>
    >({
      op: 'andHaving',
      pred: Builder.buildPredicateForOp(andHavingQ),
    });
  }

  public orHaving(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.HavingCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    condition: Types.PredicateResolverInput<TSources, TDefaultColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithHaving<TState>
  > {
    const orHavingResolved =
      this.resolvePredicateInput<
        Types.SelectedColumn<TSources, TDefaultColumns>
      >(condition);
    const orHavingQ: Types.SqlQuery =
      typeof orHavingResolved === 'string'
        ? InternalQueryEngine.makeStaticSqlQuery(orHavingResolved)
        : orHavingResolved;

    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithHaving<TState>
    >({
      op: 'orHaving',
      pred: Builder.buildPredicateForOp(orHavingQ),
    });
  }

  public orderBy(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.OrderByCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    column: Types.OrderableColumn<
      TSources,
      TDefaultColumns,
      TSelectedColumns
    >,
    direction?: Types.OrderDirection,
    nullOrder?: Types.NullOrder,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithOrderBy<TState>
  >;

  public orderBy(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.OrderByCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    columns: Array<
      Types.OrderableColumn<TSources, TDefaultColumns, TSelectedColumns>
    >,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithOrderBy<TState>
  >;

  public orderBy(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.OrderByCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    columnOrColumns: Types.OrderByInput<
      TSources,
      TDefaultColumns,
      TSelectedColumns
    >,
    direction?: Types.OrderDirection,
    nullOrder?: Types.NullOrder,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithOrderBy<TState>,
    TConnection
  > {
    if (Array.isArray(columnOrColumns)) {
      return this.cloneWithPendingOp<
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        Types.WithOrderBy<TState>
      >({
        op: 'orderByColumns',
        cols: columnOrColumns,
      });
    }

    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithOrderBy<TState>
    >({
      op: 'orderBy',
      col: columnOrColumns,
      direction,
      nullOrder,
    });
  }

  public thenOrderBy(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.OrderByCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
    column: Types.OrderableColumn<
      TSources,
      TDefaultColumns,
      TSelectedColumns
    >,
    direction?: Types.OrderDirection,
    nullOrder?: Types.NullOrder,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithOrderBy<TState>
  > {
    return this.orderBy(column, direction, nullOrder);
  }

  public limit(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.LimitCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState
      >,
    count: number,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithLimit<TState>
  > {
    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithLimit<TState>
    >({
      op: 'limit',
      count,
    });
  }

  public offset(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.OffsetCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState
      >,
    count: number,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithOffset<TState>
  > {
    return this.cloneWithPendingOp<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      Types.WithOffset<TState>
    >({
      op: 'offset',
      count,
    });
  }

  public forUpdate(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.SelectLockCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    TState,
    TConnection
  > {
    return this.applyDirectMutation<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState
    >((handle) => RuntimeBridge.runtimeBuilderForUpdate(handle));
  }

  public forShare(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.SelectLockCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    TState,
    TConnection
  > {
    return this.applyDirectMutation<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState
    >((handle) => RuntimeBridge.runtimeBuilderForShare(handle));
  }

  public noWait(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.SelectLockCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    TState,
    TConnection
  > {
    return this.applyDirectMutation<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState
    >((handle) => RuntimeBridge.runtimeBuilderNoWait(handle));
  }

  public skipLocked(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.SelectLockCallableDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    TState,
    TConnection
  > {
    return this.applyDirectMutation<
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState
    >((handle) => RuntimeBridge.runtimeBuilderSkipLocked(handle));
  }

  public union(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.CompleteSelectDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState
      >,
    query: Types.QueryOperand<TSelectedColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithCompound<TState>
  > {
    return query instanceof Database
      ? this.cloneWithPendingOp<
          TRegisteredSources,
          TSources,
          TDefaultColumns,
          TSelectedColumns,
          Types.WithCompound<TState>
        >({
          op: 'unionBuilder',
          rhsBuilder: query as Types.AnyDatabaseInstance,
        })
      : this.cloneWithPendingOp<
          TRegisteredSources,
          TSources,
          TDefaultColumns,
          TSelectedColumns,
          Types.WithCompound<TState>
        >({
          op: 'union',
          query: RuntimeBridge.serializeQuery(query.query),
        });
  }

  public unionAll(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.CompleteSelectDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState
      >,
    query: Types.QueryOperand<TSelectedColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithCompound<TState>
  > {
    return query instanceof Database
      ? this.cloneWithPendingOp<
          TRegisteredSources,
          TSources,
          TDefaultColumns,
          TSelectedColumns,
          Types.WithCompound<TState>
        >({
          op: 'unionAllBuilder',
          rhsBuilder: query as Types.AnyDatabaseInstance,
        })
      : this.cloneWithPendingOp<
          TRegisteredSources,
          TSources,
          TDefaultColumns,
          TSelectedColumns,
          Types.WithCompound<TState>
        >({
          op: 'unionAll',
          query: RuntimeBridge.serializeQuery(query.query),
        });
  }

  public intersect(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.CompleteSelectDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState
      >,
    query: Types.QueryOperand<TSelectedColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithCompound<TState>
  > {
    return query instanceof Database
      ? this.cloneWithPendingOp<
          TRegisteredSources,
          TSources,
          TDefaultColumns,
          TSelectedColumns,
          Types.WithCompound<TState>
        >({
          op: 'intersectBuilder',
          rhsBuilder: query as Types.AnyDatabaseInstance,
        })
      : this.cloneWithPendingOp<
          TRegisteredSources,
          TSources,
          TDefaultColumns,
          TSelectedColumns,
          Types.WithCompound<TState>
        >({
          op: 'intersect',
          query: RuntimeBridge.serializeQuery(query.query),
        });
  }

  public except(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.CompleteSelectDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState
      >,
    query: Types.QueryOperand<TSelectedColumns>,
  ): Database<
    TSchema,
    TRegisteredSources,
    TSources,
    TDefaultColumns,
    TSelectedColumns,
    Types.WithCompound<TState>
  > {
    return query instanceof Database
      ? this.cloneWithPendingOp<
          TRegisteredSources,
          TSources,
          TDefaultColumns,
          TSelectedColumns,
          Types.WithCompound<TState>
        >({
          op: 'exceptBuilder',
          rhsBuilder: query as Types.AnyDatabaseInstance,
        })
      : this.cloneWithPendingOp<
          TRegisteredSources,
          TSources,
          TDefaultColumns,
          TSelectedColumns,
          Types.WithCompound<TState>
        >({
          op: 'except',
          query: RuntimeBridge.serializeQuery(query.query),
        });
  }

  public clear(): Database<
    TSchema,
    Types.EmptySourceColumnMap,
    Types.EmptySourceColumnMap,
    never,
    never,
    Types.InitialBuilderState,
    TConnection
  > {
    return new Database<
      TSchema,
      Types.EmptySourceColumnMap,
      Types.EmptySourceColumnMap,
      never,
      never,
      Types.InitialBuilderState,
      TConnection
    >({
      dialect: this.dialect,
      connection: this.connection,
    } as Types.DatabaseConfig<TSchema, TConnection>);
  }

  public as<TAlias extends string>(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState
    >,
    alias: TAlias,
  ): Types.AliasAccess<
    TState,
    Types.AliasedQuery<TAlias, TSelectedColumns>
  > {
    let materialized: Types.AnyAliasedQuery | undefined;
    return InternalQueryEngine.createLazyAliasedQuery<
      TAlias,
      TSelectedColumns
    >(this as unknown as Types.AnyDatabaseInstance, alias, () => {
      materialized ??= RuntimeBridge.runtimeBuilderAs(
        this.committedHandle,
        alias,
      ) as Types.AnyAliasedQuery;
      return materialized;
    }) as Types.AliasAccess<
      TState,
      Types.AliasedQuery<TAlias, TSelectedColumns>
    >;
  }

  public get query(): Types.CompleteQueryAccess<
    TState,
    Types.SqlQuery,
    'query'
  > {
    return RuntimeBridge.runtimeBuilderGetQuery(
      this.committedHandle,
    ) as Types.CompleteQueryAccess<TState, Types.SqlQuery, 'query'>;
  }

  public get text(): Types.CompleteQueryAccess<TState, string, 'text'> {
    return RuntimeBridge.runtimeBuilderText(
      this.committedHandle,
    ) as Types.CompleteQueryAccess<TState, string, 'text'>;
  }

  public get raw(): Types.CompleteQueryAccess<TState, string, 'raw'> {
    return RuntimeBridge.runtimeBuilderRaw(
      this.committedHandle,
    ) as Types.CompleteQueryAccess<TState, string, 'raw'>;
  }

  public compile(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.CompleteQueryDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
  ): Types.CompiledQuery {
    if (this._compiledResult !== undefined) return this._compiledResult;
    const handle = this.committedHandle;
    const bundle = Diagnostics.QueryDiagnostics.measureDiagnosticsStep(
      Diagnostics.DiagnosticsCollector.Compiler(),
      'compileBundleMs',
      () => RuntimeBridge.runtimeBuilderCompileBundle(handle),
    );
    this._compiledResult = {
      text: bundle.text,
      raw: bundle.raw,
      values: bundle.values,
    };
    return this._compiledResult;
  }

  public execute(
    this: Database<
      TSchema,
      TRegisteredSources,
      TSources,
      TDefaultColumns,
      TSelectedColumns,
      TState,
      TConnection
    > &
      Types.CompleteQueryDatabaseType<
        TSchema,
        TRegisteredSources,
        TSources,
        TDefaultColumns,
        TSelectedColumns,
        TState,
        TConnection
      >,
  ): TConnection extends Types.DatabaseConnection<infer TResult>
    ? Promise<Awaited<TResult>>
    : never {
    if (!this.connection) {
      throw new Error(
        'Cannot execute query without a configured connection.',
      );
    }

    const query = RuntimeBridge.runtimeBuilderGetQuery(
      this.committedHandle,
    );
    return Promise.resolve(
      this.connection.execute(query),
    ) as TConnection extends Types.DatabaseConnection<infer TResult>
      ? Promise<Awaited<TResult>>
      : never;
  }
}

export { Database };
export default Database;
