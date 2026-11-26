//! Progress and cancellation for one long-running engine call.
//!
//! The shell creates a [`Job`], hands it to the call (which runs on a
//! background thread) and keeps a reference on the UI side. The UI polls
//! [`Job::progress`] to draw a progress bar and calls [`Job::cancel`] when
//! the user stops the work. Polling works the same in every UI framework
//! and never calls into UI code from the engine's worker threads.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// How far a job has got. `total` is 0 while the engine is still counting
/// the work (e.g. listing a folder).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
}

impl Progress {
    /// Share of the work done, from 0.0 to 1.0, or `None` while the total
    /// is still unknown (draw an indeterminate bar then).
    #[allow(clippy::cast_precision_loss)] // display only
    pub fn fraction(self) -> Option<f64> {
        (self.total > 0).then(|| self.done.min(self.total) as f64 / self.total as f64)
    }
}

/// Shared between one engine call and the UI. Thread-safe; wrap it in an
/// `Arc` to share it. Single-use: create a new one for every call, since
/// a cancelled job stays cancelled.
#[derive(Debug, Default)]
pub struct Job {
    cancelled: AtomicBool,
    done: AtomicU64,
    total: AtomicU64,
}

impl Job {
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks the call to stop soon. It then returns
    /// [`CoreError::Cancelled`](crate::CoreError::Cancelled) and changes
    /// nothing.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub fn progress(&self) -> Progress {
        Progress {
            done: self.done.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
        }
    }

    /// Called by the engine once it knows how much work there is.
    pub(crate) fn start(&self, total: usize) {
        self.done.store(0, Ordering::Relaxed);
        self.total
            .store(u64::try_from(total).unwrap_or(u64::MAX), Ordering::Relaxed);
    }

    /// Called by the engine after each unit of work (from any thread).
    pub(crate) fn advance(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_progress_once_the_total_is_known() {
        let job = Job::new();
        assert_eq!(job.progress().fraction(), None);

        job.start(4);
        job.advance();
        assert_eq!(job.progress(), Progress { done: 1, total: 4 });
        assert_eq!(job.progress().fraction(), Some(0.25));
    }

    #[test]
    fn fraction_never_exceeds_one() {
        let progress = Progress { done: 5, total: 4 };
        assert_eq!(progress.fraction(), Some(1.0));
    }

    #[test]
    fn cancel_is_visible_to_other_threads() {
        let job = std::sync::Arc::new(Job::new());
        let worker = std::sync::Arc::clone(&job);
        std::thread::spawn(move || worker.cancel()).join().unwrap();
        assert!(job.is_cancelled());
    }
}
