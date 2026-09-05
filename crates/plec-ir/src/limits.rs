//! Documented resource limits for untrusted inputs.
//!
//! Artifact bytes, runtime values, snapshots, fetch responses, source
//! modules, and execution work arrive from sources that may be hostile
//! (browser tabs, build workers, fetched responses). Every limit here is
//! enforced fail-closed at a boundary before unbounded allocation or
//! recursion can happen. Values are generous: they must never reject
//! legitimate compiled output, only pathological inputs. See
//! docs/security-limits.md for the boundary-by-boundary contract.

/// Maximum JSON byte length of a component application artifact or lazy
/// graph payload decoded at the WASM boundary.
pub const MAX_ARTIFACT_JSON_BYTES: usize = 16 * 1024 * 1024;

/// Maximum JSON byte length of a route manifest decoded at the WASM
/// boundary.
pub const MAX_MANIFEST_JSON_BYTES: usize = 1024 * 1024;

/// Maximum JSON byte length of an SSR bootstrap snapshot decoded at the
/// WASM boundary.
pub const MAX_SNAPSHOT_JSON_BYTES: usize = 4 * 1024 * 1024;

/// Maximum JSON byte length of host inputs supplied by the embedding host.
pub const MAX_HOST_INPUT_JSON_BYTES: usize = 1024 * 1024;

/// Maximum nesting depth of a decoded runtime value (constant pool entry,
/// host input, fetch response, snapshot export value).
pub const MAX_VALUE_DEPTH: usize = 64;

/// Maximum total number of nodes (elements across arrays and record
/// entries) in one decoded runtime value tree.
pub const MAX_VALUE_NODES: usize = 100_000;

/// Maximum byte length of one string inside a runtime value tree.
pub const MAX_VALUE_STRING_BYTES: usize = 1024 * 1024;

/// Maximum number of components in one application artifact.
pub const MAX_COMPONENT_COUNT: usize = 5_000;

/// Maximum length of any per-component IR collection (nodes, strings,
/// constants, expressions, actions, bindings, ...).
pub const MAX_COMPONENT_COLLECTION_LEN: usize = 100_000;

/// Maximum byte length of one string in a component string pool.
pub const MAX_COMPONENT_STRING_BYTES: usize = 64 * 1024;

/// Maximum instruction count of one expression program.
pub const MAX_EXPRESSION_INSTRUCTIONS: usize = 10_000;

/// Maximum instruction count of one action program.
pub const MAX_ACTION_INSTRUCTIONS: usize = 10_000;

/// Maximum JS-object nesting depth walked while normalizing a host-supplied
/// payload before JSON round-tripping (bounds normalize recursion itself,
/// independent of the JSON parser's depth guard).
pub const MAX_DECODE_JS_DEPTH: usize = 128;

/// Maximum bytes of one source module file read by the compiler.
pub const MAX_SOURCE_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Maximum import-chain depth of the compiler source graph traversal.
pub const MAX_IMPORT_DEPTH: usize = 128;

/// Maximum number of modules in one compiled source graph.
pub const MAX_MODULE_COUNT: usize = 4_096;

/// Maximum interpreter steps for one typed expression evaluation
/// (including nested Filter/Map predicate work).
pub const MAX_EXPRESSION_STEPS: usize = 100_000;

/// Maximum Filter/Map nesting depth inside one expression evaluation.
pub const MAX_EVAL_NESTING: usize = 32;

/// Maximum interpreter steps across one action continuation run.
pub const MAX_ACTION_STEPS: usize = 1_000_000;

/// Maximum depth of action `Call` continuation stacking.
pub const MAX_CALL_DEPTH: usize = 64;

/// Maximum total reaction executions in one top-level reaction drain.
pub const MAX_REACTION_STEPS: usize = 10_000;

/// Maximum nesting depth of reaction drains. Reaction actions re-enter
/// refresh_state and drain again, so this bounds the recursion itself.
pub const MAX_REACTION_DRAIN_DEPTH: usize = 32;

/// Maximum byte length of a fetch response body decoded into a runtime
/// value.
pub const MAX_FETCH_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// Maximum byte length of an inbound HTTP request body buffered by the
/// dev server.
pub const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;

/// Maximum number of routes in a manifest.
pub const MAX_MANIFEST_ROUTES: usize = 2_048;

/// Maximum number of route-chain entries, loader outcomes, structure
/// graphs, and public exports in one SSR snapshot.
pub const MAX_SNAPSHOT_ENTRIES: usize = 2_048;

/// Maximum number of loop keys recorded for one loop node in a snapshot.
pub const MAX_SNAPSHOT_LOOP_KEYS: usize = 10_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_leave_headroom_above_legitimate_output() {
        // The compiled demo application is far below every cap; if a limit
        // ever shrinks below real output, this guards the floor.
        assert!(MAX_ARTIFACT_JSON_BYTES >= 1024 * 1024);
        assert!(MAX_COMPONENT_COUNT >= 64);
        assert!(MAX_COMPONENT_COLLECTION_LEN >= 1_000);
        assert!(MAX_IMPORT_DEPTH >= 16);
        assert!(MAX_MODULE_COUNT >= 64);
    }
}
