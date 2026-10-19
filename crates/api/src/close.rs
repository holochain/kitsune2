//! Helper for implementing an idempotent `close`.

use std::sync::atomic::{AtomicBool, Ordering};

/// Runs a close procedure to completion at most once.
///
/// Shared by the `close` implementations of [Kitsune](crate::Kitsune),
/// [Transport](crate::Transport) and [TxImp](crate::TxImp), so that they all
/// follow the same contract:
///
/// - A close counts as requested as soon as a [CloseOnce::run] future is
///   first polled, see [CloseOnce::is_requested].
/// - Concurrent callers of [CloseOnce::run] wait until the running close
///   finishes.
/// - Once a close has completed, further calls return right away.
/// - If a close is cancelled before it completes, the next call runs the
///   procedure again, so every step of it must be safe to repeat.
#[derive(Debug, Default)]
pub struct CloseOnce {
    /// Whether a close has been requested.
    requested: AtomicBool,
    /// Whether a close has completed.
    done: futures::lock::Mutex<bool>,
}

impl CloseOnce {
    /// Creates a [CloseOnce] whose close has not run yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs `close` unless a previous close has already completed.
    ///
    /// Marks the close as requested right away, before waiting for a close
    /// that is already running. The close counts as completed only once
    /// `close` has run to the end.
    pub async fn run<F>(&self, close: F)
    where
        F: Future<Output = ()>,
    {
        self.requested.store(true, Ordering::SeqCst);
        let mut done = self.done.lock().await;
        if *done {
            return;
        }
        close.await;
        *done = true;
    }

    /// Returns whether a close has been requested.
    ///
    /// This is `true` once a [CloseOnce::run] future has been polled, also while
    /// that close is still running and after it was cancelled. Use it to
    /// reject new work once closing has started.
    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    /// Returns whether a close has completed.
    ///
    /// Takes `&mut self` so it can be called without waiting, for example
    /// from a `Drop` implementation.
    pub fn is_done(&mut self) -> bool {
        *self.done.get_mut()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn run_completes_once() {
        let mut close = CloseOnce::new();
        let runs = AtomicUsize::new(0);
        assert!(!close.is_requested());

        close
            .run(async {
                runs.fetch_add(1, Ordering::SeqCst);
            })
            .await;
        close
            .run(async {
                runs.fetch_add(1, Ordering::SeqCst);
            })
            .await;

        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert!(close.is_requested());
        assert!(close.is_done());
    }

    #[tokio::test]
    async fn concurrent_callers_wait_for_the_running_close() {
        let close = CloseOnce::new();
        let runs = AtomicUsize::new(0);
        let (release_send, release_recv) = tokio::sync::oneshot::channel();

        let first = close.run(async {
            release_recv.await.unwrap();
            runs.fetch_add(1, Ordering::SeqCst);
        });
        let second = async {
            close
                .run(async {
                    runs.fetch_add(1, Ordering::SeqCst);
                })
                .await;
            // The second caller only returns once the first close finished.
            assert_eq!(runs.load(Ordering::SeqCst), 1);
        };
        let release = async {
            tokio::task::yield_now().await;
            release_send.send(()).unwrap();
        };

        tokio::join!(first, second, release);
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn run_after_cancelled_close_runs_again() {
        let mut close = CloseOnce::new();
        let runs = AtomicUsize::new(0);

        tokio::time::timeout(
            Duration::from_millis(10),
            close.run(std::future::pending::<()>()),
        )
        .await
        .expect_err("the first close must be cancelled");
        assert!(close.is_requested());
        assert!(!close.is_done());

        close
            .run(async {
                runs.fetch_add(1, Ordering::SeqCst);
            })
            .await;

        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert!(close.is_done());
    }
}
