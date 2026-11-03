//! TASK-05-002: battery throttling hook (PLAN §10 "pause on metered/battery",
//! §3 rule 4). While the shared power state says "on battery", the queue
//! pauses dequeueing entirely; the moment AC returns, queued jobs proceed at
//! full speed — no lost work, no polling busy loop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use fotobase_jobs::{job, BatteryHook, Job, Outcome, Priority, Queue, ThrottleHook};

/// A job that records "I ran" in a shared flag.
fn ran_job(flag: Arc<AtomicBool>) -> Job<u32> {
    job(move |_| {
        let flag = flag.clone();
        async move {
            flag.store(true, Ordering::SeqCst);
            Outcome::Done(1u32)
        }
    })
}

#[tokio::test]
async fn ac_state_runs_jobs_immediately() {
    let hook = Arc::new(BatteryHook::new(false));
    let queue = Queue::with_throttle(4, hook.clone() as Arc<dyn ThrottleHook>);
    let flag = Arc::new(AtomicBool::new(false));
    let handle = queue.enqueue("k", Priority::Background, ran_job(flag.clone())).await.unwrap();
    assert!(matches!(*handle.result().await, Outcome::Done(1)));
    assert!(flag.load(Ordering::SeqCst));
}

#[tokio::test]
async fn battery_state_pauses_dequeueing() {
    let hook = Arc::new(BatteryHook::new(true));
    let queue = Queue::with_throttle(4, hook.clone() as Arc<dyn ThrottleHook>);
    let flag = Arc::new(AtomicBool::new(false));
    let handle = queue.enqueue("k", Priority::Background, ran_job(flag.clone())).await.unwrap();

    // Still parked on battery: the job must not run.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!flag.load(Ordering::SeqCst), "job ran while on battery");

    // AC returns: the paused job proceeds without re-enqueueing.
    hook.set_on_battery(false);
    assert!(matches!(*handle.result().await, Outcome::Done(1)));
    assert!(flag.load(Ordering::SeqCst));
}

#[tokio::test]
async fn flipping_to_battery_mid_run_pauses_the_next_job() {
    let hook = Arc::new(BatteryHook::new(false));
    let queue = Queue::with_throttle(1, hook.clone() as Arc<dyn ThrottleHook>);
    let first = queue
        .enqueue("a", Priority::Background, ran_job(Arc::new(AtomicBool::new(false))))
        .await
        .unwrap();
    assert!(matches!(*first.result().await, Outcome::Done(1)));

    hook.set_on_battery(true);
    let flag = Arc::new(AtomicBool::new(false));
    let second = queue.enqueue("b", Priority::Background, ran_job(flag.clone())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!flag.load(Ordering::SeqCst), "queued job ran on battery");

    hook.set_on_battery(false);
    assert!(matches!(*second.result().await, Outcome::Done(1)));
}
