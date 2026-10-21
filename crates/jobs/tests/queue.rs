//! TDD tests for TASK-01-009: bounded priority queue with cancellation,
//! coalescing and a throttling hook (PLAN §3 rule 4).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use fotobase_jobs::{job, Job, JobContext, Outcome, Priority, Queue, ThrottleHook};

type Record = Arc<tokio::sync::Mutex<Vec<&'static str>>>;
type Gate = Arc<AtomicBool>;

/// A job that sets `started` once it is occupying the worker, then waits until
/// `gate` opens (simulating heavy work), then records `name`. An atomic flag
/// (not a `Notify`) is used so jobs that are still queued when the gate opens
/// also observe it.
fn blocking_job(name: &'static str, gate: Gate, record: Record) -> Job<&'static str> {
    job(move |_ctx: JobContext| {
        let gate = gate.clone();
        let record = record.clone();
        async move {
            while !gate.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            record.lock().await.push(name);
            Outcome::Done(name)
        }
    })
}

/// Like [`blocking_job`], but signals `started` as soon as the job body runs,
/// letting a test pin down the moment this job occupies the single worker.
fn blocking_job_with_started(
    name: &'static str,
    started: Arc<AtomicBool>,
    gate: Gate,
    record: Record,
) -> Job<&'static str> {
    job(move |_ctx: JobContext| {
        let started = started.clone();
        let gate = gate.clone();
        let record = record.clone();
        async move {
            started.store(true, Ordering::SeqCst);
            while !gate.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            record.lock().await.push(name);
            Outcome::Done(name)
        }
    })
}

/// Spins (with yields) until `flag` becomes true.
async fn wait_until(flag: &AtomicBool) {
    while !flag.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

#[tokio::test]
async fn dequeue_order_is_priority_respecting() {
    let queue: Queue<&'static str> = Queue::new(16);
    let gate: Gate = Arc::new(AtomicBool::new(false));
    let record: Record = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    // The first job starts immediately and blocks the single worker, so every
    // later enqueue lands in the pending heap before anything else runs.
    // Wait until it has actually *started* — enqueuing before that would race
    // with the worker's dequeue, and the heap would then (correctly) order the
    // visible jobs ahead of `first`.
    let first_started = Arc::new(AtomicBool::new(false));
    let first = queue
        .enqueue(
            "first",
            Priority::Background,
            blocking_job_with_started("first", first_started.clone(), gate.clone(), record.clone()),
        )
        .await
        .expect("enqueue");
    wait_until(&first_started).await;

    let mut handles = Vec::new();
    for (name, prio) in [
        ("bg1", Priority::Background),
        ("bg2", Priority::Background),
        ("vis1", Priority::Visible),
        ("vis2", Priority::Visible),
        ("bg3", Priority::Background),
    ] {
        handles.push(
            queue
                .enqueue(name, prio, blocking_job(name, gate.clone(), record.clone()))
                .await
                .expect("enqueue"),
        );
    }

    gate.store(true, Ordering::SeqCst); // release the blocked job and all queued ones
    first.result().await;
    for h in &handles {
        h.result().await;
    }

    let order = record.lock().await.clone();
    assert_eq!(order, vec!["first", "vis1", "vis2", "bg1", "bg2", "bg3"]);
}

#[tokio::test]
async fn queue_is_bounded() {
    let queue: Queue<&'static str> = Queue::new(2);
    let gate: Gate = Arc::new(AtomicBool::new(false));
    let record: Record = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    let h1 = queue
        .enqueue("a", Priority::Visible, blocking_job("a", gate.clone(), record.clone()))
        .await
        .expect("enqueue");
    queue
        .try_enqueue("b", Priority::Visible, blocking_job("b", gate.clone(), record.clone()))
        .expect("second job fits");

    // Bounded: a third pending job must be rejected by try_enqueue...
    let err = queue
        .try_enqueue("c", Priority::Visible, blocking_job("c", gate.clone(), record.clone()))
        .unwrap_err();
    assert!(matches!(err, fotobase_jobs::EnqueueError::Full));

    // ...and `enqueue` waits for space instead of growing without bound.
    let q = queue.clone();
    let record_for_waiter = record.clone();
    let gate_for_waiter = gate.clone();
    let waiter = tokio::spawn(async move {
        q.enqueue("c", Priority::Visible, blocking_job("c", gate_for_waiter, record_for_waiter))
            .await
            .expect("enqueue after space frees")
    });

    gate.store(true, Ordering::SeqCst);
    h1.result().await;
    waiter.await.expect("task").result().await;

    let order = record.lock().await.clone();
    assert_eq!(order, vec!["a", "b", "c"]);
}

#[tokio::test]
async fn cancel_running_job_leaves_no_partial_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let tmp = dir.path().join("thumb.jpg.tmp");
    let final_path = dir.path().join("thumb.jpg");

    let queue: Queue<String> = Queue::new(4);
    let tmp_w = tmp.clone();
    let handle = queue
        .enqueue("photo-1", Priority::Visible, {
            job(move |ctx: JobContext| {
                let tmp = tmp_w.clone();
                async move {
                    // Simulate a thumbnail generator: write to a temp file,
                    // then finalize. Cancellation must leave no partial entry.
                    std::fs::write(&tmp, b"partial bytes").expect("write tmp");
                    ctx.cancelled().await; // observe cancellation mid-flight
                    let _ = std::fs::remove_file(&tmp); // clean the partial write
                    if ctx.is_cancelled() {
                        Outcome::Cancelled
                    } else {
                        std::fs::write(tmp.with_extension("jpg"), b"final").expect("write final");
                        Outcome::Done("done".into())
                    }
                }
            })
        })
        .await
        .expect("enqueue");

    // Wait until the job has written its partial output, then cancel.
    while !tmp.exists() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    handle.cancel();

    let outcome = handle.result().await;
    assert_eq!(*outcome, Outcome::Cancelled);
    assert!(!final_path.exists(), "no finalized output after cancel");
    assert!(!tmp.exists(), "partial temp file cleaned up");
}

#[tokio::test]
async fn duplicate_jobs_coalesce_with_in_flight_job() {
    let queue: Queue<&'static str> = Queue::new(8);
    let gate: Gate = Arc::new(AtomicBool::new(false));
    let runs = Arc::new(AtomicUsize::new(0));
    let gate2 = gate.clone();
    let runs2 = runs.clone();

    let handle1 = queue
        .enqueue(
            "photo-1",
            Priority::Visible,
            job(move |_ctx| {
                let gate = gate2.clone();
                let runs = runs2.clone();
                async move {
                    while !gate.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(2)).await;
                    }
                    runs.fetch_add(1, Ordering::SeqCst);
                    Outcome::Done("rendered")
                }
            }),
        )
        .await
        .expect("enqueue");

    // Same key while the first job is queued/in flight: coalesces.
    let handle2 = queue
        .enqueue("photo-1", Priority::Visible, job(|_ctx| async { Outcome::Done("never runs") }))
        .await
        .expect("coalesced enqueue");

    gate.store(true, Ordering::SeqCst);
    let r1 = handle1.result().await;
    let r2 = handle2.result().await;
    assert_eq!(*r1, Outcome::Done("rendered"));
    assert_eq!(*r2, Outcome::Done("rendered"), "waiter shares the in-flight result");
    assert_eq!(runs.load(Ordering::SeqCst), 1, "job body ran exactly once");
}

#[tokio::test]
async fn cancel_queued_job_prevents_execution() {
    let queue: Queue<&'static str> = Queue::new(8);
    let gate: Gate = Arc::new(AtomicBool::new(false));
    let record: Record = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    let running = queue
        .enqueue("a", Priority::Visible, blocking_job("a", gate.clone(), record.clone()))
        .await
        .expect("enqueue");
    let queued = queue
        .enqueue("b", Priority::Visible, blocking_job("b", gate.clone(), record.clone()))
        .await
        .expect("enqueue");

    queued.cancel();
    gate.store(true, Ordering::SeqCst);
    running.result().await;

    assert_eq!(*queued.result().await, Outcome::Cancelled);
    assert_eq!(*record.lock().await, vec!["a"], "cancelled queued job never ran");
}

#[tokio::test]
async fn throttle_hook_gates_dequeueing() {
    struct Gate {
        open: Arc<AtomicBool>,
    }
    impl ThrottleHook for Gate {
        fn wait_admission(
            &self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
            let open = self.open.clone();
            Box::pin(async move {
                while !open.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
        }
    }

    let open = Arc::new(AtomicBool::new(false));
    let queue: Queue<&'static str> = Queue::with_throttle(8, Arc::new(Gate { open: open.clone() }));
    let record: Record = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let record_in_job = record.clone();

    let handle = queue
        .enqueue(
            "a",
            Priority::Visible,
            job(move |_ctx| {
                let record = record_in_job.clone();
                async move {
                    record.lock().await.push("a");
                    Outcome::Done("a")
                }
            }),
        )
        .await
        .expect("enqueue");

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(record.lock().await.is_empty(), "throttled queue must not run jobs");

    open.store(true, Ordering::SeqCst); // "unplug from battery"
    handle.result().await;
    assert_eq!(*record.lock().await, vec!["a"]);
}
