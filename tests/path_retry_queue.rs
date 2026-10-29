// Exercise the exact helper used by the vendored dependency without compiling
// unrelated upstream development fixtures or opening a live peer connection.
#[path = "../vendor/iroh/src/socket/remote_map/remote_state/pending_paths.rs"]
mod pending_paths;

use pending_paths::{MAX_PENDING_OPEN_PATHS, enqueue};
use std::collections::VecDeque;

#[test]
fn repeated_failures_across_connections_do_not_multiply_retry_work() {
    let mut pending = VecDeque::new();
    enqueue(&mut pending, &1);
    enqueue(&mut pending, &2);
    for _ in 0..100 {
        // The actor drains each candidate and retries it on every connection.
        // Both connections remain out of path IDs/CIDs for this test.
        for path in std::mem::take(&mut pending) {
            for _ in 0..2 {
                enqueue(&mut pending, &path);
            }
        }
        assert_eq!(pending, VecDeque::from([1, 2]));
    }
}

#[test]
fn address_churn_has_a_fixed_bound_and_keeps_recent_candidates() {
    let mut pending = VecDeque::new();
    for path in 0..MAX_PENDING_OPEN_PATHS * 10 {
        enqueue(&mut pending, &path);
        assert!(pending.len() <= MAX_PENDING_OPEN_PATHS);
    }
    assert_eq!(pending.front(), Some(&(MAX_PENDING_OPEN_PATHS * 9)));
    assert_eq!(pending.back(), Some(&(MAX_PENDING_OPEN_PATHS * 10 - 1)));
}

#[test]
fn duplicate_at_capacity_does_not_evict_another_path() {
    let mut pending: VecDeque<_> = (0..MAX_PENDING_OPEN_PATHS).collect();
    let expected = pending.clone();
    enqueue(&mut pending, &0);
    assert_eq!(pending, expected);
}
