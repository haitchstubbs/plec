pub mod backend;
pub mod builder;
pub mod dialect;
pub mod error;
pub mod expr_node;
pub mod expressions;
pub mod sql;
pub mod types;
pub mod validation;

pub use backend::{backend_for_dialect, backend_for_dialect_name, BackendRef, SqlBackend};
pub use dialect::{classify, DialectFeature, DialectPolicy, DialectWarning};
pub use error::{QueryError, UnsupportedFeature, ValidationError};
pub use types::RenderResult;
pub use validation::{validator_for_dialect, DialectValidator};

pub use builder::canonical::{builder_canonical_ir_hash, builder_canonical_ir_json};
pub use builder::methods::*;
pub use builder::render::{
    builder_as,
    // typed (no-JSON-string) variants used by native/wasm bindings:
    builder_as_typed,
    builder_compile_bundle,
    builder_compile_bundle_typed,
    builder_conflict_target_kind_typed,
    builder_insert_columns,
    builder_insert_columns_typed,
    builder_query,
    builder_query_typed,
    builder_raw,
    builder_selected_columns,
    builder_selected_columns_typed,
    builder_text,
    builder_values,
    builder_values_typed,
    AliasedQueryData,
    CompiledBundle,
};
pub use dialect::{normalize_dialect_name, Dialect};
pub use expressions::*;
#[allow(deprecated)]
pub use sql::{
    compile_postgres, compile_query, compile_query_dialect, identifier, join, raw, ref_identifier,
    sql, to_query_fragment,
};
pub use types::{Primitive, SqlIdentifier, SqlQuery, SqlRaw, SqlValue, WindowOrderItem};

// ─── EngineSnapshot (backward-compat stub) ────────────────────────────────────

pub struct EngineSnapshot {
    dialect: String,
}

impl EngineSnapshot {
    pub fn new(dialect: Option<&str>) -> Self {
        let dialect = dialect
            .map(Dialect::parse)
            .unwrap_or_default()
            .as_str()
            .to_string();
        Self { dialect }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        // Minimal snapshot for API compatibility with existing TS callers.
        Ok(format!(
            r#"{{"dialect":"{}","stage":"start","artifacts":{{"text":"","raw":"","values_json":[]}}}}"#,
            self.dialect
        ))
    }
}

// ─── Version ─────────────────────────────────────────────────────────────────

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
