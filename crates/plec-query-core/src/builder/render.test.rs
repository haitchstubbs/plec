use super::*;
use crate::{
    builder_from_table, builder_join_table, builder_limit, builder_new, builder_on,
    builder_select_columns, builder_where,
};

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

static BUILD_EXECUTION_COUNTS: Lazy<Mutex<HashMap<u64, usize>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static CACHE_LOOKUP_COUNTS: Lazy<Mutex<HashMap<u64, usize>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static VALIDATION_TEST_GUARD: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

static VALIDATION_EXECUTION_COUNT: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn record_cache_lookup(key: u64) {
    let mut counts = CACHE_LOOKUP_COUNTS.lock();
    let entry = counts.entry(key).or_insert(0);
    *entry += 1;
}

pub(crate) fn record_build_execution(key: u64) {
    let mut counts = BUILD_EXECUTION_COUNTS.lock();
    let entry = counts.entry(key).or_insert(0);
    *entry += 1;
}

pub(crate) fn record_validation_execution() {
    if VALIDATION_TEST_GUARD.try_lock().is_none() {
        VALIDATION_EXECUTION_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn get_or_build_query_arc_with_structural_cache(
    handle: &str,
) -> Result<Arc<SqlQuery>, String> {
    get_or_build_query_arc_internal_with_test_counts(handle, true)
}

fn get_or_build_query_arc_internal_with_test_counts(
    handle: &str,
    use_structural_cache: bool,
) -> Result<Arc<SqlQuery>, String> {
    let key = registry_parse_handle(handle)?;

    {
        record_cache_lookup(key);
    }

    if let Some(cached) = registry_try_get_cached_key(key) {
        return Ok(cached);
    }

    let parts = registry_get_cloned_key(key)?;

    if use_structural_cache && !query_parts_contain_runtime_values(&parts) {
        let canonical_fingerprint = canonical_fingerprint_for_parts(&parts).ok();

        if let Some(canonical_fingerprint) = canonical_fingerprint {
            if let Some(shared_cached) = registry_try_get_structural_cached(canonical_fingerprint) {
                registry_set_cached_key(key, Arc::clone(&shared_cached));
                return Ok(shared_cached);
            }

            let query = build_and_cache_query_arc_with_test_counts(key, &parts)?;
            registry_set_structural_cached(canonical_fingerprint, Arc::clone(&query));
            return Ok(query);
        }

        return build_and_cache_query_arc_with_test_counts(key, &parts);
    }

    build_and_cache_query_arc_with_test_counts(key, &parts)
}

fn build_and_cache_query_arc_with_test_counts(
    key: u64,
    parts: &QueryParts,
) -> Result<Arc<SqlQuery>, String> {
    {
        record_build_execution(key);
    }

    let query = Arc::new(build_query(parts)?);
    registry_set_cached_key(key, Arc::clone(&query));
    Ok(query)
}

pub(crate) fn test_reset_build_execution_count() {
    BUILD_EXECUTION_COUNTS.lock().clear();
    CACHE_LOOKUP_COUNTS.lock().clear();
    VALIDATION_EXECUTION_COUNT.store(0, Ordering::Relaxed);
}

pub(crate) fn test_build_execution_count_for_handle(handle: &str) -> Result<usize, String> {
    let key = registry_parse_handle(handle)?;

    let count = BUILD_EXECUTION_COUNTS.lock().get(&key).copied().unwrap_or(0);

    Ok(count)
}

pub(crate) fn test_cache_lookup_count_for_handle(handle: &str) -> Result<usize, String> {
    let key = registry_parse_handle(handle)?;

    let count = CACHE_LOOKUP_COUNTS.lock().get(&key).copied().unwrap_or(0);

    Ok(count)
}

fn validation_execution_count() -> usize {
    VALIDATION_EXECUTION_COUNT.load(Ordering::Relaxed)
}

fn build_simple_render_handle() -> String {
    let handle = builder_new(Some("postgres"));
    let handle = builder_from_table(&handle, "users".to_string()).expect("from should build");
    let handle = builder_select_columns(&handle, r#"["id","name"]"#.to_string())
        .expect("select should build");
    builder_where(
        &handle,
        r#"{"text":"\"users\".\"active\" = ?","raw":"users.active = true","values":[true]}"#
            .to_string(),
    )
    .expect("where should build")
}

#[test]
fn validation_cache_reuses_successful_validation_for_same_handle_state() {
    let _guard = VALIDATION_TEST_GUARD.lock();
    test_reset_build_execution_count();
    let handle = build_simple_render_handle();

    let first = registry_get_parts(&handle).expect("parts should exist");
    build_query(&first).expect("first render should succeed");
    assert_eq!(validation_execution_count(), 1);

    let second = registry_get_parts(&handle).expect("parts should exist");
    build_query(&second).expect("second render should succeed");
    assert_eq!(validation_execution_count(), 1);
}

#[test]
fn validation_cache_invalidates_after_mutation() {
    let _guard = VALIDATION_TEST_GUARD.lock();
    test_reset_build_execution_count();
    let handle = build_simple_render_handle();

    let first = registry_get_parts(&handle).expect("parts should exist");
    build_query(&first).expect("first render should succeed");
    assert_eq!(validation_execution_count(), 1);

    let mutated = builder_limit(&handle, 5).expect("limit should build");
    let second = registry_get_parts(&mutated).expect("mutated parts should exist");
    build_query(&second).expect("mutated render should succeed");
    assert_eq!(validation_execution_count(), 2);
}

#[test]
fn validation_cache_reuses_pending_join_validation_errors() {
    let _guard = VALIDATION_TEST_GUARD.lock();
    test_reset_build_execution_count();

    let handle = builder_new(Some("sqlite"));
    let handle = builder_from_table(&handle, "users".to_string()).expect("from should build");
    let handle = builder_select_columns(&handle, r#"["users.id"]"#.to_string())
        .expect("select should build");
    let handle = builder_join_table(&handle, "RIGHT JOIN".to_string(), "teams".to_string())
        .expect("join should build");
    let handle = builder_on(
        &handle,
        r#"{"text":"\"users\".\"team_id\" = \"teams\".\"id\"","raw":"users.team_id = teams.id","values":[]}"#
            .to_string(),
    )
    .expect("on should build");

    let first = registry_get_parts(&handle).expect("parts should exist");
    let first_err = build_query(&first).expect_err("first render should fail validation");
    assert_eq!(validation_execution_count(), 1);

    let second = registry_get_parts(&handle).expect("parts should exist");
    let second_err = build_query(&second).expect_err("second render should fail validation");
    assert_eq!(validation_execution_count(), 1);
    assert_eq!(first_err, second_err);
    assert!(first_err.contains("ValidationError"));
    assert!(first_err.contains("right_join"));
}
