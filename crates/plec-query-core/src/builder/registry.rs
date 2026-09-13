use crate::error::BuilderError;
use dashmap::DashMap;
use lru::LruCache;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::{QueryParts, ValidationCache};
use crate::dialect::Dialect;
use crate::types::SqlQuery;

// ─── Registry storage ─────────────────────────────────────────────────────────

static REGISTRY: Lazy<DashMap<u64, QueryParts>> = Lazy::new(DashMap::new);
// Const construction: panics at compile time if the literal is ever changed to zero.
// nosemgrep: rust-unwrap-in-production
const DEFAULT_STRUCTURAL_CACHE_CAPACITY: NonZeroUsize = NonZeroUsize::new(1024).unwrap();
static STRUCTURAL_CACHE: Lazy<Mutex<LruCache<u64, Arc<SqlQuery>>>> =
    Lazy::new(|| Mutex::new(LruCache::new(DEFAULT_STRUCTURAL_CACHE_CAPACITY)));

// ─── Handle generation ────────────────────────────────────────────────────────

static CTR: AtomicU64 = AtomicU64::new(1);

fn generate_handle() -> u64 {
    CTR.fetch_add(1, Ordering::Relaxed)
}

fn parse_handle_typed(id: &str) -> Result<u64, BuilderError> {
    id.parse::<u64>()
        .map_err(|_| BuilderError::InvalidHandle(id.to_string()))
}

fn parse_handle(id: &str) -> Result<u64, String> {
    parse_handle_typed(id).map_err(|err| err.to_string())
}

pub fn registry_parse_handle(id: &str) -> Result<u64, String> {
    parse_handle(id)
}

/// Explicit eviction for stale handles. Not called on every registry_new;
/// invoke from a background task or explicit cleanup path.
pub fn registry_evict_stale() {
    // No TTL logic without UUIDs — FinalizationRegistry handles cleanup.
    // This is a no-op stub kept for explicit-call compatibility.
}

// ─── Public registry API ──────────────────────────────────────────────────────

pub fn registry_new(dialect: Option<&str>) -> String {
    let key = generate_handle();
    let parts = QueryParts {
        dialect: dialect.map(Dialect::parse).unwrap_or_default(),
        ..QueryParts::default()
    };
    REGISTRY.insert(key, parts);
    key.to_string()
}

pub fn registry_clone(id: &str) -> Result<String, String> {
    let key = parse_handle(id)?;
    let new_key = generate_handle();
    let mut parts = REGISTRY
        .get(&key)
        .map(|entry| entry.clone())
        .ok_or_else(|| BuilderError::HandleNotFound(id.to_string()).to_string())?;
    parts.cached_query = None;
    REGISTRY.insert(new_key, parts);
    Ok(new_key.to_string())
}

pub fn registry_drop(id: &str) {
    if let Ok(key) = parse_handle(id) {
        REGISTRY.remove(&key);
    }
}

pub fn registry_get_cloned(id: &str) -> Result<QueryParts, String> {
    let key = parse_handle(id)?;
    registry_get_cloned_key(key)
}

pub fn registry_get_cloned_key(key: u64) -> Result<QueryParts, String> {
    REGISTRY
        .get(&key)
        .map(|entry| entry.clone())
        .ok_or_else(|| BuilderError::HandleNotFound(key.to_string()).to_string())
}

pub fn registry_get_dialect(id: &str) -> Dialect {
    let Ok(key) = parse_handle(id) else {
        return Dialect::default();
    };
    REGISTRY
        .get(&key)
        .map(|p| p.dialect.clone())
        .unwrap_or_default()
}

pub fn registry_try_get_cached(id: &str) -> Option<Arc<SqlQuery>> {
    let key = parse_handle(id).ok()?;
    registry_try_get_cached_key(key)
}

pub fn registry_try_get_cached_key(key: u64) -> Option<Arc<SqlQuery>> {
    REGISTRY.get(&key).and_then(|p| p.cached_query.clone())
}

pub fn registry_set_cached(id: &str, query: Arc<SqlQuery>) {
    if let Ok(key) = parse_handle(id) {
        registry_set_cached_key(key, query);
    }
}

pub fn registry_set_cached_key(key: u64, query: Arc<SqlQuery>) {
    if let Some(mut parts) = REGISTRY.get_mut(&key) {
        parts.cached_query = Some(query);
    }
}

pub fn registry_try_get_structural_cached(fingerprint: u64) -> Option<Arc<SqlQuery>> {
    STRUCTURAL_CACHE.lock().get(&fingerprint).cloned()
}

pub fn registry_set_structural_cached(fingerprint: u64, query: Arc<SqlQuery>) {
    STRUCTURAL_CACHE.lock().put(fingerprint, query);
}

pub fn registry_insert(parts: QueryParts) -> String {
    let mut parts = parts;
    parts.cached_query = None;
    parts.validation_cache = Arc::new(ValidationCache::new());
    let key = generate_handle();
    REGISTRY.insert(key, parts);
    key.to_string()
}

#[cfg(test)]
pub(crate) use registry_test::{
    set_structural_cache_capacity as test_set_structural_cache_capacity,
    structural_cache_len as test_structural_cache_len,
};

#[cfg(test)]
#[path = "registry.test.rs"]
mod registry_test;
