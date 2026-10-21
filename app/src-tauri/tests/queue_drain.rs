//! TASK-01-022: `Queue::drain` — the primitive "clear cache files" leans on.
//!
//! Clearing the thumbnail cache must pause/drain the job queue for the
//! affected keys (TASK-01-022 technical spec): pending jobs are cancelled
//! and never run, the in-flight job is cancelled and waited out, and the
//! queue stays usable afterwards.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use fotobase_jobs::{job, Outcome, Queue};

/// Helper: did a handle resolve to `Cancelled`?
async fn cancelled(handle: fotobase_jobs::JobHandle<u32>) -> bool {
    matches!(*handle.result().await, Outcome::Cancelled)
}

#[tokio::test]
async fn drain_on_idle_queue_returns_immediately() {
    let q: Queue<u32> = Queue::new(4);
    q.drain().await; // must not hang
}

#[tokio::test]
async fn drain_cancels_pending_jobs_they_never_run() {
    let q: Queue<u32> = Queue::new(8);
    let runs = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    for i in 0..4 {
        let runs2 = runs.clone();
        handles.push(
            q.try_enqueue(
                format!("k{i}"),
                fotobase_jobs::Priority::Background,
                job(move |_| {
                    let runs = runs2.clone();
                    async move {
                        runs.fetch_add(1, Ordering::SeqCst);
                        Outcome::Done(i)
                    }
                }),
            )
            .expect("enqueue"),
        );
    }
    q.drain().await;
    assert_eq!(runs.load(Ordering::SeqCst), 0, "drained pending jobs must not run");
    for h in handles {
        assert!(cancelled(h).await, "pending job resolves Cancelled");
    }
}

#[tokio::test]
async fn drain_waits_for_the_in_flight_job() {
    let q: Queue<u32> = Queue::new(8);
    // A job that finishes on its own shortly after being cancelled: drain
    // must not return before the worker is idle again.
    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let started_tx = Arc::new(std::sync::Mutex::new(Some(started_tx)));
    let handle = q
        .try_enqueue(
            "slow",
            fotobase_jobs::Priority::Visible,
            job(move |_| {
                let tx = started_tx.clone();
                async move {
                    if let Some(tx) = tx.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    // Simulate a render that cannot be aborted mid-write: it
                    // completes even though a cancel already arrived.
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    Outcome::Done(1u32)
                }
            }),
        )
        .expect("enqueue");
    started_rx.await.expect("job started");
    q.drain().await;
    // The queue observed cancellation even though the body ran to completion.
    assert!(matches!(*handle.result().await, Outcome::Cancelled));
    // And the queue is idle + usable again.
    let h2 = q
        .try_enqueue(
            "next",
            fotobase_jobs::Priority::Visible,
            job(|_| async { Outcome::Done(2u32) }),
        )
        .expect("queue usable after drain");
    assert!(matches!(*h2.result().await, Outcome::Done(2)));
}

#[tokio::test]
async fn drain_cancels_coalesced_duplicates_too() {
    let q: Queue<u32> = Queue::new(8);
    let runs = Arc::new(AtomicUsize::new(0));
    let runs2 = runs.clone();
    let h1 = q
        .try_enqueue(
            "same",
            fotobase_jobs::Priority::Visible,
            job(move |_| {
                let runs = runs2.clone();
                async move {
                    runs.fetch_add(1, Ordering::SeqCst);
                    Outcome::Done(1u32)
                }
            }),
        )
        .expect("enqueue");
    // Same key: coalesces onto the queued job.
    let h2 = q
        .try_enqueue(
            "same",
            fotobase_jobs::Priority::Visible,
            job(|_| async { Outcome::Done(9u32) }),
        )
        .expect("coalesce");
    q.drain().await;
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    assert!(cancelled(h1).await);
    assert!(cancelled(h2).await);
}
