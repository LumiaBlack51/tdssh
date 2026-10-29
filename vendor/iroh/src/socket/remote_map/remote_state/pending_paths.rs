use std::collections::VecDeque;

pub(super) const MAX_PENDING_OPEN_PATHS: usize = 64;

/// Queue a path for the next retry round. Several connections can fail for the
/// same path in one round, so only retain one entry per candidate.
pub(super) fn enqueue<T: Clone + PartialEq>(queue: &mut VecDeque<T>, path: &T) {
    if queue.contains(path) {
        return;
    }
    // Also bound genuinely distinct candidates during prolonged address churn.
    if queue.len() >= MAX_PENDING_OPEN_PATHS {
        queue.pop_front();
    }
    queue.push_back(path.clone());
}
