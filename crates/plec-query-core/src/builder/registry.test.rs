use super::*;

pub(crate) fn reset_structural_cache() {
    let mut guard = STRUCTURAL_CACHE.lock();
    guard.resize(DEFAULT_STRUCTURAL_CACHE_CAPACITY);
    guard.clear();
}

pub(crate) fn structural_cache_len() -> usize {
    STRUCTURAL_CACHE.lock().len()
}

pub(crate) fn set_structural_cache_capacity(capacity: usize) {
    let normalized = capacity.max(1);
    let normalized = NonZeroUsize::new(normalized).expect("normalized capacity is non-zero");
    STRUCTURAL_CACHE.lock().resize(normalized);
}

#[test]
fn reset_structural_cache_clears_cache() {
    reset_structural_cache();

    assert_eq!(structural_cache_len(), 0);
}
