//! Keyed loop reorder planning.
//!
//! Reconciliation needs to know which surviving rows are already in their
//! final relative position so the placement pass can leave them untouched.
//! A row is "stable" when it belongs to the longest strictly increasing
//! subsequence (LIS) of the previous order, read in the desired order. Rows
//! outside the LIS are relocated exactly once each by the right-to-left
//! anchored placement pass, so an unchanged order yields zero DOM writes and
//! a single-row move stays O(1) regardless of list size.

use std::collections::HashMap;

/// Compute, for each entry of `desired`, whether the keyed row can stay in
/// place (`true`) or must be relocated (`false`). Keys absent from `previous`
/// are new rows and are never stable. `previous` must mirror the live DOM row
/// order; duplicate keys are impossible (reconciliation rejects them).
pub(crate) fn keyed_reorder_plan(previous: &[String], desired: &[String]) -> Vec<bool> {
    let previous_index: HashMap<&str, usize> = previous
        .iter()
        .enumerate()
        .map(|(index, key)| (key.as_str(), index))
        .collect();
    let sequence: Vec<(usize, usize)> = desired
        .iter()
        .enumerate()
        .filter_map(|(position, key)| {
            previous_index
                .get(key.as_str())
                .map(|&index| (index, position))
        })
        .collect();
    let mut stable = vec![false; desired.len()];
    if sequence.is_empty() {
        return stable;
    }
    // Patience LIS over the previous indices, with parent pointers so the
    // subsequence itself can be marked. Deterministic: ties replace the
    // earliest equivalent tail.
    let mut tails: Vec<usize> = Vec::with_capacity(sequence.len());
    let mut parents: Vec<Option<usize>> = vec![None; sequence.len()];
    for (cursor, &(value, _)) in sequence.iter().enumerate() {
        let slot = match tails.binary_search_by(|&tail| sequence[tail].0.cmp(&value)) {
            Ok(slot) | Err(slot) => slot,
        };
        if slot > 0 {
            parents[cursor] = Some(tails[slot - 1]);
        }
        if slot == tails.len() {
            tails.push(cursor);
        } else {
            tails[slot] = cursor;
        }
    }
    let mut cursor = tails.last().copied();
    while let Some(index) = cursor {
        stable[sequence[index].1] = true;
        cursor = parents[index];
    }
    stable
}

#[cfg(test)]
mod tests {
    use super::keyed_reorder_plan;

    fn plan(previous: &[&str], desired: &[&str]) -> Vec<bool> {
        let previous = previous
            .iter()
            .map(|key| (*key).to_string())
            .collect::<Vec<_>>();
        keyed_reorder_plan(
            &previous,
            &desired
                .iter()
                .map(|key| (*key).to_string())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn unchanged_order_keeps_every_row_stable() {
        assert_eq!(
            plan(&["a", "b", "c"], &["a", "b", "c"]),
            vec![true, true, true]
        );
    }

    #[test]
    fn adjacent_swap_relocates_exactly_one_row() {
        let stable = plan(&["a", "b", "c"], &["a", "c", "b"]);
        assert_eq!(stable.iter().filter(|stable| !**stable).count(), 1);
    }

    #[test]
    fn single_move_in_large_list_relocates_one_row() {
        let stable = plan(
            &["a", "b", "c", "d", "e", "f", "g", "h"],
            &["a", "b", "d", "c", "e", "f", "g", "h"],
        );
        assert_eq!(stable.iter().filter(|stable| !**stable).count(), 1);
    }

    #[test]
    fn move_to_front_relocates_only_the_moved_row() {
        let stable = plan(&["a", "b", "c", "d"], &["d", "a", "b", "c"]);
        assert_eq!(stable, vec![false, true, true, true]);
    }

    #[test]
    fn full_reversal_relocates_all_but_one_row() {
        let stable = plan(&["a", "b", "c", "d", "e"], &["e", "d", "c", "b", "a"]);
        assert_eq!(stable.iter().filter(|stable| !**stable).count(), 4);
    }

    #[test]
    fn new_rows_are_never_stable_and_removals_disappear() {
        // "b" removed, "x" inserted, "a"/"c" keep relative order.
        let stable = plan(&["a", "b", "c"], &["x", "a", "c"]);
        assert_eq!(stable, vec![false, true, true]);
    }

    #[test]
    fn empty_desired_yields_no_plan() {
        assert_eq!(plan(&["a", "b"], &[]), Vec::<bool>::new());
    }

    #[test]
    fn empty_previous_makes_every_row_new() {
        assert_eq!(plan(&[], &["a", "b"]), vec![false, false]);
    }

    #[test]
    fn lis_is_strictly_increasing_not_non_decreasing() {
        // Only one of two equal-position rows can stay: previous order must
        // win for exactly one row, the other must relocate.
        let stable = plan(&["a", "b"], &["b", "b"]);
        assert_eq!(stable.iter().filter(|stable| !**stable).count(), 1);
    }
}
